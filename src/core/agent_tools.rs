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
            ("j", e.generation_queue.iter().map(|x| x.id).collect()),
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
            "jobs":e.generation_queue.iter().rev().take(15).map(job).collect::<Vec<_>>(),
            "counts":{"assets":e.project.assets.len(),"clips":e.project.clips.len(),"tracks":e.project.tracks.len()},
            "hint":"Use inspect for one handle. Lists bounded to 40 assets/clips, 20 tracks, 30 providers and 15 recent jobs."
        }))
    }

    pub fn inspect(&self, e: &EditorState, v: &Value) -> Result<Value, String> {
        let id = self.id(v, "handle", "")?;
        let result = if let Some(a) = e.project.assets.iter().find(|a| a.id == id) {
            json!({"asset":asset(a),"generation":e.project.generative_configs.get(&id).map(|g|json!({"provider":g.provider_id,"inputs":g.inputs,"reference_slots":g.reference_slots,"batch":g.batch,"active_version":g.active_version,"versions":g.versions.iter().rev().take(20).map(|r|json!({"version":r.version})).collect::<Vec<_>>(),"version_count":g.versions.len()}))})
        } else if let Some(c) = e.project.clips.iter().find(|c| c.id == id) {
            json!(c)
        } else if let Some(t) = e.project.tracks.iter().find(|t| t.id == id) {
            json!(t)
        } else if let Some(p) = e.provider_entries.iter().find(|p| p.id == id) {
            json!({"id":p.id,"name":p.name,"ready":ready(p),"output_type":p.output_type,"inputs":p.inputs,"canvas":p.canvas,"timing":p.timing})
        } else if let Some(j) = e.generation_queue.iter().find(|j| j.id == id) {
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
                        for (k, value) in inputs {
                            let input = if value.get("asset").is_some() {
                                let id = self.id(value, "asset", "a")?;
                                if let Some(version) = optional(value, "version") {
                                    json!({"type":"generation_ref","asset_id":id,"version":version})
                                } else {
                                    json!({"type":"asset_ref","asset_id":id,"pinned":false})
                                }
                            } else {
                                json!({"type":"literal","value":value})
                            };
                            mapped.insert(k.clone(), input);
                        }
                        patch["inputs"] = Value::Object(mapped);
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
        tool("generation","Configure inputs, start asynchronously (optional context clip), read status, or activate a version. Inspect provider first. inputs maps exact provider field names to literal values or {asset:a1,version:optional}. Generation sidecars use normal immediate persistence.",json!({"action":{"enum":["configure","start","status","activate_version"]},"asset":s,"provider":s,"clip":s,"version":s,"inputs":{"type":"object"}}),json!(["action"])),
        tool("save_project","Save the current project document and clear its unsaved-project changes.",json!({}),json!([])),
    ];
    if provider.capabilities.image_input {
        tools.push(tool("look","Inspect a rendered frame or contact sheet. source is timeline, clip c1 or asset a1. Seconds are source-relative. Default is 8 annotated frames, no playhead movement.",json!({"source":s,"version":s,"mode":{"enum":["frame","cutsheet"]},"time":n,"times":{"type":"array","items":n,"maxItems":8}}),json!([])));
    }
    if provider.capabilities.video_input {
        tools.push(tool("watch_video","Watch the actual encoded video of one whole video asset, active or specified version. Native video input; not still images. Maximum 32 MiB.",json!({"asset":s,"version":s}),json!(["asset"])));
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Asset, Project, ProviderConnection, ProviderEntry, ProviderOutputType};

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
    fn capabilities_control_tool_vocabulary() {
        let mut p = AgentProviderEntry::default();
        assert_eq!(schemas(&p).len(), 6);
        p.capabilities.image_input = true;
        assert!(schemas(&p).iter().any(|t| t["function"]["name"] == "look"));
        assert!(!schemas(&p)
            .iter()
            .any(|t| t["function"]["name"] == "watch_video"));
        p.capabilities.video_input = true;
        assert_eq!(schemas(&p).len(), 8);
        p.capabilities.image_input = false;
        assert_eq!(schemas(&p).len(), 7);
        assert!(!schemas(&p).iter().any(|t| t["function"]["name"] == "look"));
        assert!(serde_json::to_string(&schemas(&p)).unwrap().len() < 6000);
    }
}
