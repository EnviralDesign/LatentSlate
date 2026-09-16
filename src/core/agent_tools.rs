//! Curated project vocabulary for Chat; UUIDs and the automation API stay host-side.
use super::automation::{AutomationCommand, CaptureSource};
use crate::{editor::EditorState, state::AgentProviderEntry};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Default)]
pub struct Handles(Vec<(String, Uuid)>);

impl Handles {
    pub fn sync(&mut self, e: &EditorState) {
        for (prefix, ids) in [
            (
                "a",
                e.project.assets.iter().map(|x| x.id).collect::<Vec<_>>(),
            ),
            ("c", e.project.clips.iter().map(|x| x.id).collect()),
            ("t", e.project.tracks.iter().map(|x| x.id).collect()),
            ("p", e.provider_entries.iter().map(|x| x.id).collect()),
            (
                "j",
                e.generation_queue
                    .iter()
                    .filter(|job| job_belongs_to_current_project(e, job))
                    .map(|job| job.id)
                    .collect(),
            ),
        ] {
            for id in ids {
                if !self.0.iter().any(|(_, existing)| *existing == id) {
                    let n = self.0.iter().filter(|(h, _)| h.starts_with(prefix)).count() + 1;
                    self.0.push((format!("{prefix}{n}"), id));
                }
            }
        }
    }

    pub fn id(&self, v: &Value, field: &str, prefix: &str) -> Result<Uuid, String> {
        let h = string(v, field)?;
        self.0
            .iter()
            .find(|(key, _)| key == h && key.starts_with(prefix))
            .map(|(_, id)| *id)
            .ok_or_else(|| format!("Unknown {field} handle. Read project_context."))
    }

    pub fn compact(&self, value: Value) -> Value {
        match value {
            Value::String(mut s) => {
                for (h, id) in &self.0 {
                    s = s.replace(&id.to_string(), h);
                }
                Value::String(s)
            }
            Value::Array(a) => Value::Array(a.into_iter().map(|v| self.compact(v)).collect()),
            Value::Object(o) => {
                Value::Object(o.into_iter().map(|(k, v)| (k, self.compact(v))).collect())
            }
            v => v,
        }
    }

    pub fn context(&self, e: &EditorState) -> Value {
        self.compact(json!({
            "project":e.project.name, "settings":e.project.settings, "playhead_seconds":e.current_time,
            "dirty":e.project_dirty, "duration_seconds":e.project.duration(),
            "selection":{"assets":e.selection.asset_ids,"clips":e.selection.clip_ids,"tracks":e.selection.track_ids},
            "assets":e.project.assets.iter().take(40).map(asset).collect::<Vec<_>>(),
            "clips":e.project.clips.iter().take(40).map(|c|json!({"id":c.id,"asset":c.asset_id,"track":c.track_id,"start":c.start_time,"duration":c.duration})).collect::<Vec<_>>(),
            "tracks":e.project.tracks.iter().take(20).map(|t|json!({"id":t.id,"name":t.name,"kind":t.track_type})).collect::<Vec<_>>(),
            "providers":e.provider_entries.iter().filter(|p|ready(p)&&e.provider_in_project_scope(p.id)).take(30).map(|p|json!({"id":p.id,"name":p.name,"output_type":p.output_type})).collect::<Vec<_>>(),
            "jobs":e.generation_queue.iter().rev().filter(|job|job_belongs_to_current_project(e,job)).take(15).map(job).collect::<Vec<_>>(),
            "counts":{"assets":e.project.assets.len(),"clips":e.project.clips.len(),"tracks":e.project.tracks.len()},
            "hint":"Use inspect for one handle. Lists bounded to 40 assets/clips, 20 tracks, 30 providers and 15 recent jobs."
        }))
    }

    pub fn inspect(&self, e: &EditorState, v: &Value) -> Result<Value, String> {
        let id = self.id(v, "handle", "")?;
        let result = if let Some(a) = e.project.assets.iter().find(|a| a.id == id) {
            json!({"asset":asset(a),"generation":e.project.generative_configs.get(&id).map(|g|json!({"provider":g.provider_id,"inputs":g.inputs,"media_bindings":g.media_bindings,"reference_slots":g.reference_slots,"batch":g.batch,"active_version":g.active_version,"versions":g.versions.iter().rev().take(20).map(|r|json!({"version":r.version})).collect::<Vec<_>>(),"version_count":g.versions.len()}))})
        } else if let Some(c) = e.project.clips.iter().find(|c| c.id == id) {
            json!(c)
        } else if let Some(t) = e.project.tracks.iter().find(|t| t.id == id) {
            json!(t)
        } else if let Some(p) = e.provider_entries.iter().find(|p| p.id == id) {
            json!({"id":p.id,"name":p.name,"ready":ready(p),"output_type":p.output_type,"inputs":p.inputs,"canvas":p.canvas,"timing":p.timing})
        } else if let Some(j) = e
            .generation_queue
            .iter()
            .find(|j| j.id == id && job_belongs_to_current_project(e, j))
        {
            job(j)
        } else {
            return Err("Item no longer exists.".into());
        };
        Ok(self.compact(result))
    }

    pub fn jobs(&self, e: &EditorState, asset: Option<Uuid>) -> Value {
        self.compact(json!(e
            .generation_queue
            .iter()
            .rev()
            .filter(|job| job_belongs_to_current_project(e, job))
            .filter(|j| asset.is_none_or(|id| j.asset_id == id))
            .take(15)
            .map(job)
            .collect::<Vec<_>>()))
    }

    pub fn source(&self, v: &Value) -> Result<CaptureSource, String> {
        let h = v
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("timeline");
        if h == "timeline" {
            Ok(CaptureSource::Timeline)
        } else if h.starts_with('t') {
            Ok(CaptureSource::Track {
                track_id: self.id(v, "source", "t")?,
            })
        } else if h.starts_with('c') {
            Ok(CaptureSource::Clip {
                clip_id: self.id(v, "source", "c")?,
            })
        } else {
            Ok(CaptureSource::Asset {
                asset_id: self.id(v, "source", "a")?,
                version: optional(v, "version"),
            })
        }
    }

    pub fn command(&self, name: &str, v: &Value) -> Result<AutomationCommand, String> {
        let mut out = match name {
            "save_project" => json!({"type":"save_project"}),
            "timeline_edit" => match string(v, "action")? {
                "place" => {
                    json!({"type":"add_asset_to_timeline","asset_id":self.id(v,"asset","a")?,"time":v.get("time"),"duration_seconds":v.get("duration")})
                }
                "move" => {
                    json!({"type":"move_clip","clip_id":self.id(v,"clip","c")?,"start_time":v.get("time")})
                }
                "resize" => {
                    json!({"type":"resize_clip","clip_id":self.id(v,"clip","c")?,"start_time":v.get("time"),"duration":v.get("duration")})
                }
                "add_track" => {
                    json!({"type":"add_track","track_type":string(v,"track_type")?,"name":v.get("name")})
                }
                _ => return Err("Unknown timeline action.".into()),
            },
            "asset_edit" => match string(v, "action")? {
                "import" => json!({"type":"import_asset","path":string(v,"path")?}),
                "rename" => {
                    json!({"type":"rename_asset","asset_id":self.id(v,"asset","a")?,"name":string(v,"name")?})
                }
                "create_generative" => {
                    json!({"type":"create_generative_asset","name":v.get("name"),"output_type":string(v,"output_type")?,"duration_seconds":v.get("duration"),"fps":v.get("fps")})
                }
                _ => return Err("Unknown asset action.".into()),
            },
            "generation" => match string(v, "action")? {
                "configure" => {
                    let mut patch = json!({});
                    if v.get("provider").is_some() {
                        patch["provider_id"] = json!(self.id(v, "provider", "p")?);
                    }
                    if let Some(inputs) = v.get("inputs") {
                        let inputs = inputs.as_object().ok_or("inputs must be an object")?;
                        let mut mapped = serde_json::Map::new();
                        let mut media = serde_json::Map::new();
                        for (k, value) in inputs {
                            if value.get("asset").is_some() {
                                let id = self.id(value, "asset", "a")?;
                                media.insert(k.clone(), json!({
                                    "source":{"type":"project_asset","asset_id":id,"version":optional(value,"version")},
                                    "sample":{"type":"auto"},"coverage":"strict"
                                }));
                            } else {
                                mapped.insert(k.clone(), json!({"type":"literal","value":value}));
                            }
                        }
                        patch["inputs"] = Value::Object(mapped);
                        patch["media_bindings"] = Value::Object(media);
                    }
                    json!({"type":"set_generative_config","asset_id":self.id(v,"asset","a")?,"patch":patch})
                }
                "activate_version" => {
                    json!({"type":"set_active_generation_version","asset_id":self.id(v,"asset","a")?,"version":string(v,"version")?})
                }
                "start" => {
                    json!({"type":"start_generation","asset_id":self.id(v,"asset","a")?,"wait":false})
                }
                _ => return Err("Unknown generation action.".into()),
            },
            _ => return Err("Tool unavailable.".into()),
        };
        if name == "timeline_edit" && v.get("track").is_some() {
            out["track_id"] = json!(self.id(v, "track", "t")?);
        }
        if name == "generation" && v["action"] == "start" && v.get("clip").is_some() {
            out["context_clip_id"] = json!(self.id(v, "clip", "c")?);
        }
        serde_json::from_value(out).map_err(|_| "Missing or invalid action arguments.".into())
    }
}

fn job_belongs_to_current_project(e: &EditorState, job: &crate::state::GenerationJob) -> bool {
    e.project
        .assets
        .iter()
        .any(|asset| asset.id == job.asset_id)
        && e.project
            .project_path
            .as_ref()
            .is_some_and(|root| job.folder_path.starts_with(root))
}

fn ready(p: &crate::state::ProviderEntry) -> bool {
    !matches!(
        p.connection,
        crate::state::ProviderConnection::LatentSlateEngine {
            available: false,
            ..
        }
    )
}
fn asset(a: &crate::state::Asset) -> Value {
    use crate::state::AssetKind::*;
    let (kind, active) = match &a.kind {
        Video { .. } => ("video", None),
        Image { .. } => ("image", None),
        Audio { .. } => ("audio", None),
        GenerativeVideo { active_version, .. } => ("generative_video", active_version.as_ref()),
        GenerativeImage { active_version, .. } => ("generative_image", active_version.as_ref()),
        GenerativeAudio { active_version, .. } => ("generative_audio", active_version.as_ref()),
    };
    json!({"id":a.id,"name":a.name,"kind":kind,"active_version":active,"duration_seconds":a.duration_seconds})
}
fn job(j: &crate::state::GenerationJob) -> Value {
    json!({"id":j.id,"asset":j.asset_id,"status":j.status,"progress":j.progress_overall,"version":j.version})
}
pub fn string<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("Missing {k}."))
}
pub fn optional(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

pub fn schemas(provider: &AgentProviderEntry) -> Vec<Value> {
    let s = json!({"type":"string"});
    let n = json!({"type":"number","minimum":0});
    let tool = |name, description, properties, required| json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}});
    let mut tools=vec![
        tool("project_context","Read compact project state and stable handles before editing.",json!({}),json!([])),
        tool("inspect","Read one asset, clip, track, generation provider or job by handle. Provider details include input names and types.",json!({"handle":s}),json!(["handle"])),
        tool("timeline_edit","Place an asset, move or resize a clip (seconds), or add a track. Mutations stay unsaved.",json!({"action":{"enum":["place","move","resize","add_track"]},"asset":s,"clip":s,"track":s,"time":n,"duration":n,"track_type":{"enum":["Video","Audio"]},"name":s}),json!(["action"])),
        tool("asset_edit","Import a user-specified file, rename, create a generative asset, or extract a rendered still from timeline/c1/a1 at time seconds.",json!({"action":{"enum":["import","rename","create_generative","extract_still"]},"path":s,"asset":s,"name":s,"source":s,"time":n,"version":s,"output_type":{"enum":["image","video","audio"]},"duration":n,"fps":n}),json!(["action"])),
        tool("generation","Configure inputs, start asynchronously (optional context clip), read status, or activate a version. Inspect provider first. inputs maps exact provider field names to literal values or {asset:a1,version:optional}, which locks that project asset/version as the media source rather than following the timeline. Generation sidecars use normal immediate persistence.",json!({"action":{"enum":["configure","start","status","activate_version"]},"asset":s,"provider":s,"clip":s,"version":s,"inputs":{"type":"object"}}),json!(["action"])),
        tool("save_project","Save the current project document and clear its unsaved-project changes.",json!({}),json!([])),
    ];
    if provider.capabilities.image_input {
        tools.push(tool("look","Inspect an original image asset (mode asset), a rendered frame, or an annotated contact sheet. source: asset a1, timeline (visible viewer composite), track t1 (isolated, even if hidden), or clip c1 (composite at clip-relative time). time is seconds; frame is a zero-based project-FPS index, mutually exclusive with time. Timeline/track time is absolute, asset/clip time is relative. Default mode: asset for image assets, frame when time/frame supplied, otherwise 8-frame cutsheet. Does not move the playhead.",json!({"source":s,"version":s,"mode":{"enum":["asset","frame","cutsheet"]},"time":n,"frame":{"type":"integer","minimum":0},"times":{"type":"array","items":n,"maxItems":8}}),json!([])));
    }
    if provider.capabilities.video_input && provider.connection.supports_native_video() {
        tools.push(tool("watch_video","Watch native video: source a1 (video asset), timeline (visible viewer composite), or t1 (isolated video track, even if hidden). Optional version for assets. start/end are seconds, end exclusive: asset-relative or absolute timeline time. Timeline/track require both; omit both for a whole asset only if <=12 seconds. Maximum 12 seconds, 32 MiB; request a smaller range if exceeded. Prepared as silent H.264, up to 320x320 preserving aspect. Backend controls sampling FPS; exact frame inspection uses look.",json!({"source":s,"asset":s,"version":s,"start":n,"end":n}),json!([])));
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Asset, Project, ProviderConnection, ProviderEntry, ProviderOutputType};

    #[test]
    fn chat_jobs_follow_project_ownership_without_changing_the_global_queue() {
        let mut e = EditorState::new();
        e.provider_entries.clear();
        e.project = Project::new("Project B");
        let root_a = std::env::temp_dir().join("chat-project-a");
        let root_b = std::env::temp_dir().join("chat-project-b");
        e.project.project_path = Some(root_b.clone());
        let asset = Asset::new_generative_image("Current image", "generated/image/current".into());
        let asset_id = asset.id;
        e.project.assets.push(asset);
        let make_job = |asset_id, root: &std::path::Path| crate::state::GenerationJob {
            authoring_snapshot: None,
            lab_submission: None,
            id: Uuid::new_v4(),
            created_at: chrono::Utc::now(),
            status: crate::state::GenerationJobStatus::Succeeded,
            progress_overall: None,
            progress_stage: None,
            attempts: 0,
            next_attempt_at: None,
            provider: ProviderEntry::new(
                "Fixture",
                ProviderOutputType::Image,
                ProviderConnection::CustomHttp {
                    base_url: "http://127.0.0.1".into(),
                    api_key: None,
                },
            ),
            output_type: ProviderOutputType::Image,
            asset_id,
            clip_id: None,
            asset_label: "Image".into(),
            folder_path: root.join("generated/image/current"),
            inputs: Default::default(),
            inputs_snapshot: Default::default(),
            media_bindings_snapshot: Default::default(),
            resolved_media_inputs: Default::default(),
            seed_advance: None,
            version: Some("v1".into()),
            lab_node_id: None,
            activate_on_success: true,
            error: None,
        };
        let foreign = make_job(Uuid::new_v4(), &root_a);
        let cloned_asset = make_job(asset_id, &root_a);
        let sibling = make_job(asset_id, &root_b.with_file_name("chat-project-b-other"));
        let current = make_job(asset_id, &root_b);
        let current_id = current.id;
        e.generation_queue = vec![foreign, cloned_asset, sibling, current];
        let original_queue = e.generation_queue.clone();
        let mut handles = Handles::default();
        handles.sync(&e);

        assert_eq!(
            handles.id(&json!({"handle":"j1"}), "handle", "j").unwrap(),
            current_id
        );
        assert!(handles.id(&json!({"handle":"j2"}), "handle", "j").is_err());
        assert_eq!(handles.context(&e)["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(handles.jobs(&e, None).as_array().unwrap().len(), 1);
        assert_eq!(handles.jobs(&e, Some(asset_id))[0]["asset"], "a1");
        assert_eq!(
            handles.inspect(&e, &json!({"handle":"j1"})).unwrap()["asset"],
            "a1"
        );
        assert!(handles.inspect(&e, &json!({"handle":"a1"})).is_ok());

        let asset = e.project.assets.remove(0);
        handles.sync(&e);
        assert_eq!(handles.context(&e)["jobs"], json!([]));
        assert_eq!(handles.jobs(&e, Some(asset_id)), json!([]));
        assert!(handles.inspect(&e, &json!({"handle":"j1"})).is_err());

        e.project.assets.push(asset);
        e.project.project_path = None;
        let mut unsaved_handles = Handles::default();
        unsaved_handles.sync(&e);
        assert!(unsaved_handles
            .id(&json!({"handle":"j1"}), "handle", "j")
            .is_err());
        assert_eq!(handles.context(&e)["jobs"], json!([]));
        assert_eq!(handles.jobs(&e, None), json!([]));
        assert!(handles.inspect(&e, &json!({"handle":"j1"})).is_err());
        assert!(handles.inspect(&e, &json!({"handle":"a1"})).is_ok());
        assert_eq!(e.generation_queue, original_queue);
    }

    #[test]
    fn handles_survive_reorder_and_removal_and_context_is_compact() {
        let mut e = EditorState::new();
        e.provider_entries.clear();
        e.project = Project::new("Chat fixture");
        for i in 0..40 {
            e.project.assets.push(Asset::new_image(
                format!("Image {i}"),
                format!("images/{i}.png").into(),
            ));
        }
        let first = e.project.assets[0].id;
        let mut h = Handles::default();
        h.sync(&e);
        assert_eq!(h.id(&json!({"asset":"a1"}), "asset", "a").unwrap(), first);
        e.project.assets.remove(0);
        e.project.assets.reverse();
        let new = Asset::new_image("New", "images/new.png".into());
        let new_id = new.id;
        e.project.assets.push(new);
        h.sync(&e);
        assert_eq!(h.id(&json!({"asset":"a41"}), "asset", "a").unwrap(), new_id);
        assert!(h.inspect(&e, &json!({"handle":"a1"})).is_err());
        assert!(h.id(&json!({"asset":new_id}), "asset", "a").is_err());
        let context = h.context(&e).to_string();
        assert!(context.len() < 12_000, "{} bytes", context.len());
        assert!(!context.contains(&new_id.to_string()));
        let mut reset = Handles::default();
        reset.sync(&e);
        assert_eq!(
            reset.id(&json!({"asset":"a1"}), "asset", "a").unwrap(),
            e.project.assets[0].id
        );
    }

    #[test]
    fn curated_edits_use_real_editor_and_explicit_save() {
        let root = std::env::temp_dir().join(format!("latentslate-chat-{}", Uuid::new_v4()));
        let mut e = EditorState::new();
        e.provider_entries.clear();
        e.project = Project::new("Chat fixture");
        e.project.project_path = Some(root.clone());
        e.save().unwrap();
        let mut h = Handles::default();
        h.sync(&e);
        let apply = |e: &mut EditorState, h: &mut Handles, name, v| {
            let cmd = h.command(name, &v).unwrap();
            let response = e.apply_automation_command(&cmd);
            assert!(response.ok, "{:?}", response.message);
            h.sync(e);
            response
        };
        apply(
            &mut e,
            &mut h,
            "asset_edit",
            json!({"action":"create_generative","output_type":"image","name":"Concept"}),
        );
        let provider = ProviderEntry::new(
            "Fixture",
            ProviderOutputType::Image,
            ProviderConnection::OpenAiImage {
                api_key: Some("never-in-context".into()),
                model: "fixture".into(),
                base_url: None,
            },
        );
        e.provider_entries.push(provider);
        h.sync(&e);
        apply(
            &mut e,
            &mut h,
            "generation",
            json!({"action":"configure","asset":"a1","provider":"p1","inputs":{"prompt":"A blue square"}}),
        );
        assert!(!h
            .inspect(&e, &json!({"handle":"p1"}))
            .unwrap()
            .to_string()
            .contains("never-in-context"));
        assert!(h
            .inspect(&e, &json!({"handle":"a1"}))
            .unwrap()
            .to_string()
            .contains("A blue square"));
        assert!(matches!(
            h.command("generation", &json!({"action":"start","asset":"a1"}))
                .unwrap(),
            AutomationCommand::StartGeneration { wait: false, .. }
        ));
        apply(
            &mut e,
            &mut h,
            "timeline_edit",
            json!({"action":"place","asset":"a1","time":1.0,"duration":4.0}),
        );
        apply(
            &mut e,
            &mut h,
            "timeline_edit",
            json!({"action":"move","clip":"c1","time":2.0}),
        );
        apply(
            &mut e,
            &mut h,
            "timeline_edit",
            json!({"action":"resize","clip":"c1","time":2.0,"duration":3.0}),
        );
        assert_eq!(e.project.clips[0].start_time, 2.0);
        assert_eq!(e.project.clips[0].duration, 3.0);
        assert!(h
            .command("timeline_edit", &json!({"action":"delete","clip":"c1"}))
            .is_err());
        e.refresh_project_dirty_state();
        assert!(e.project_dirty);
        apply(&mut e, &mut h, "save_project", json!({}));
        assert!(!e.project_dirty);
        let reopened = Project::load(&root).unwrap();
        assert_eq!(reopened.clips.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_media_sources_replace_follow_bindings_and_resolve_off_timeline() {
        use crate::core::media_binding::{resolve_field, MediaResolveContext};
        use crate::state::{
            GenerationRecord, MediaBindingRelation, MediaBindingSource, MediaBindingSpec,
            ProviderInputField,
        };

        let root = std::env::temp_dir().join(format!("latentslate-chat-media-{}", Uuid::new_v4()));
        let mut e = EditorState::new();
        e.provider_entries.clear();
        e.project = Project::new("Chat media fixture");
        e.project.project_path = Some(root.clone());
        e.save().unwrap();
        let still = Asset::new_image("Coffee still", "coffee.png".into());
        let still_id = still.id;
        image::RgbImage::new(8, 8)
            .save(root.join("coffee.png"))
            .unwrap();
        e.project.assets.push(still);
        let source_id = e.create_generative_image().unwrap();
        let source_folder = match &e.project.find_asset(source_id).unwrap().kind {
            crate::state::AssetKind::GenerativeImage { folder, .. } => root.join(folder),
            _ => unreachable!(),
        };
        let target_id = e.create_generative_video(24.0, 24).unwrap();
        let mut provider = ProviderEntry::new(
            "Image to video fixture",
            ProviderOutputType::Video,
            ProviderConnection::CustomHttp {
                base_url: "http://127.0.0.1".into(),
                api_key: None,
            },
        );
        let field: ProviderInputField = serde_json::from_value(json!({
            "name":"start_image", "label":"Start Image", "input_type":{"type":"image"},
            "required":true, "role":"start_image"
        }))
        .unwrap();
        provider.inputs.push(field.clone());
        e.provider_entries.push(provider.clone());
        for version in ["v1", "v2"] {
            image::RgbImage::new(8, 8)
                .save(source_folder.join(format!("{version}.png")))
                .unwrap();
            e.project
                .generative_configs
                .get_mut(&source_id)
                .unwrap()
                .versions
                .push(GenerationRecord {
                    label: String::new(),
                    authoring_snapshot: None,
                    engine_execution: None,
                    version: version.into(),
                    timestamp: chrono::Utc::now(),
                    provider_id: provider.id,
                    inputs_snapshot: Default::default(),
                    media_bindings_snapshot: Default::default(),
                    resolved_media_inputs: Default::default(),
                    lab_node_id: None,
                });
        }
        e.project
            .generative_configs
            .get_mut(&source_id)
            .unwrap()
            .active_version = Some("v2".into());
        e.set_generation_provider(target_id, Some(provider.id))
            .unwrap();
        e.project
            .generative_configs
            .get_mut(&target_id)
            .unwrap()
            .media_bindings
            .insert("start_image".into(), MediaBindingSpec::follow_auto());
        e.save().unwrap();
        assert!(e.project.clips.is_empty());

        let mut h = Handles::default();
        h.sync(&e);
        let apply = |e: &mut EditorState, source: Value| {
            let command = h
                .command(
                    "generation",
                    &json!({
                        "action":"configure", "asset":"a3", "provider":"p1",
                        "inputs":{"start_image":source, "prompt":"Keep the mug"}
                    }),
                )
                .unwrap();
            let response = e.apply_automation_command(&command);
            assert!(response.ok, "{:?}", response.message);
        };
        let resolve = |e: &EditorState| {
            resolve_field(MediaResolveContext {
                project: &e.project,
                target_asset_id: Some(target_id),
                context_clip_id: None,
                field: &field,
                provider: Some(&provider),
                config: e.project.generative_config(target_id),
            })
            .unwrap()
        };
        assert!(!resolve(&e).is_ok());
        apply(&mut e, json!({"asset":"a1"}));
        let plan = resolve(&e);
        assert!(plan.is_ok(), "{:?}", plan.errors);
        assert_eq!(plan.relation, Some(MediaBindingRelation::ExplicitAsset));
        assert_eq!(plan.source_asset_id, Some(still_id));
        assert_eq!(
            plan.spec.source,
            MediaBindingSource::ProjectAsset {
                asset_id: still_id,
                version: None
            }
        );

        apply(&mut e, json!({"asset":"a2", "version":"v1"}));
        let plan = resolve(&e);
        assert!(plan.is_ok(), "{:?}", plan.errors);
        assert_eq!(plan.source_asset_id, Some(source_id));
        assert_eq!(plan.source_version.as_deref(), Some("v1"));
        assert_eq!(
            plan.source_path_absolute,
            Some(source_folder.join("v1.png"))
        );
        let inspected = h.inspect(&e, &json!({"handle":"a3"})).unwrap();
        assert_eq!(
            inspected["generation"]["media_bindings"]["start_image"],
            json!({
                "source":{"type":"project_asset","asset_id":"a2","version":"v1"},
                "sample":{"type":"auto"},"coverage":"strict"
            })
        );
        let reloaded = Project::load(&root).unwrap();
        assert_eq!(
            reloaded
                .generative_config(target_id)
                .unwrap()
                .media_bindings,
            e.project
                .generative_config(target_id)
                .unwrap()
                .media_bindings
        );

        let before = e.project.generative_config(target_id).unwrap().clone();
        assert!(h
            .command(
                "generation",
                &json!({
                    "action":"configure","asset":"a3","inputs":{"start_image":{"asset":"a99"}}
                })
            )
            .is_err());
        assert_eq!(e.project.generative_config(target_id).unwrap(), &before);
        apply(&mut e, json!({"asset":"a2", "version":"missing"}));
        assert!(!resolve(&e).is_ok());
        let start = h
            .command("generation", &json!({"action":"start","asset":"a3"}))
            .unwrap();
        assert!(!e.apply_automation_command(&start).ok);
        assert!(e.generation_queue.is_empty());
        assert!(e.project.clips.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capabilities_control_tool_vocabulary() {
        let mut p = AgentProviderEntry::default();
        assert_eq!(schemas(&p).len(), 6);
        p.capabilities.image_input = true;
        assert!(schemas(&p).iter().any(|t| t["function"]["name"] == "look"));
        assert!(!schemas(&p)
            .iter()
            .any(|t| t["function"]["name"] == "watch_video"));
        p.capabilities.video_input = true;
        assert_eq!(
            schemas(&p).len(),
            7,
            "Responses must not offer native video"
        );
        if let crate::state::AgentConnection::OpenAiCompatible { protocol, .. } = &mut p.connection
        {
            *protocol = crate::state::AgentProtocol::ChatCompletions;
        }
        assert_eq!(schemas(&p).len(), 8);
        p.capabilities.image_input = false;
        assert_eq!(schemas(&p).len(), 7);
        assert!(!schemas(&p).iter().any(|t| t["function"]["name"] == "look"));
        assert!(serde_json::to_string(&schemas(&p)).unwrap().len() < 6000);
    }
}
