use super::*;
use crate::state::{
    Asset, AssetKind, Clip, ClipImageMode, ClipTimeMode, GenerativeConfig, InputRole, InputValue,
    MediaBindingSource, MediaFramePoint, MediaSample, Project, ProviderConnection, ProviderEntry,
    ProviderInputField, ProviderInputType, ProviderOutputType, SourceFrameReference,
    TimelineTrackScope,
};
use std::path::PathBuf;
use uuid::Uuid;

fn uid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn image_field(role: Option<InputRole>) -> ProviderInputField {
    ProviderInputField {
        name: match role {
            Some(InputRole::EndImage) => "end_image".into(),
            _ => "start_image".into(),
        },
        label: match role {
            Some(InputRole::EndImage) => "End Image".into(),
            _ => "Start Image".into(),
        },
        description: None,
        input_type: ProviderInputType::Image,
        required: true,
        default: None,
        role,
        ui: None,
    }
}

fn audio_field() -> ProviderInputField {
    ProviderInputField {
        name: "audio".into(),
        label: "Audio".into(),
        description: None,
        input_type: ProviderInputType::Audio,
        required: true,
        default: None,
        role: None,
        ui: None,
    }
}

fn video_field() -> ProviderInputField {
    ProviderInputField {
        name: "video".into(),
        label: "Video".into(),
        description: None,
        input_type: ProviderInputType::Video,
        required: true,
        default: None,
        role: None,
        ui: None,
    }
}

fn provider_with(fields: Vec<ProviderInputField>) -> ProviderEntry {
    let mut provider = ProviderEntry::new(
        "Test",
        ProviderOutputType::Video,
        ProviderConnection::CustomHttp {
            base_url: "http://localhost".into(),
            api_key: None,
        },
    );
    provider.inputs = fields;
    provider
}

struct Harness {
    project: Project,
    target: Asset,
    context: Clip,
    provider: ProviderEntry,
    config: GenerativeConfig,
}

impl Harness {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ls-bind-{}", Uuid::new_v4()));
        fs_create(&root);
        let mut project = Project::new("bindings");
        project.project_path = Some(root.clone());
        project.settings.fps = 24.0;

        let target_id = uid(100);
        let folder = PathBuf::from(format!("generated/video/{target_id}"));
        fs_create(&root.join(&folder));
        write_file(&root.join(&folder).join("v1.mp4"));
        let mut target = Asset::new_generative_video("Target", folder, 24.0, 24);
        target.id = target_id;
        if let AssetKind::GenerativeVideo { active_version, .. } = &mut target.kind {
            *active_version = Some("v1".into());
        }
        target.duration_seconds = Some(1.0);
        project.assets.push(target.clone());

        let context = Clip {
            id: uid(200),
            asset_id: target_id,
            track_id: project.tracks[1].id,
            start_time: 5.0,
            duration: 1.0,
            trim_in_seconds: 0.0,
            volume: 1.0,
            label: None,
            image_mode: ClipImageMode::Still,
            time_mode: ClipTimeMode::Crop,
            transform: Default::default(),
            bridge: None,
        };
        project.clips.push(context.clone());

        let provider = provider_with(vec![image_field(Some(InputRole::StartImage))]);
        Self {
            project,
            target,
            context,
            provider,
            config: GenerativeConfig::default(),
        }
    }

    fn ctx<'a>(&'a self, field: &'a ProviderInputField) -> MediaResolveContext<'a> {
        MediaResolveContext {
            project: &self.project,
            target_asset_id: Some(self.target.id),
            context_clip_id: Some(self.context.id),
            field,
            provider: Some(&self.provider),
            config: Some(&self.config),
        }
    }

    fn add_image(&mut self, id: u128, name: &str, path: &str) -> Asset {
        let abs = self.project.project_path.as_ref().unwrap().join(path);
        if let Some(parent) = abs.parent() {
            fs_create(parent);
        }
        write_file(&abs);
        let mut asset = Asset::new_image(name, PathBuf::from(path));
        asset.id = uid(id);
        self.project.assets.push(asset.clone());
        asset
    }

    fn add_video(&mut self, id: u128, name: &str, path: &str, duration: f64) -> Asset {
        let abs = self.project.project_path.as_ref().unwrap().join(path);
        if let Some(parent) = abs.parent() {
            fs_create(parent);
        }
        write_file(&abs);
        let mut asset = Asset::new_video(name, PathBuf::from(path));
        asset.id = uid(id);
        asset.duration_seconds = Some(duration);
        self.project.assets.push(asset.clone());
        asset
    }

    fn add_audio(&mut self, id: u128, name: &str, path: &str, duration: f64) -> Asset {
        let abs = self.project.project_path.as_ref().unwrap().join(path);
        if let Some(parent) = abs.parent() {
            fs_create(parent);
        }
        write_file(&abs);
        let mut asset = Asset::new_audio(name, PathBuf::from(path));
        asset.id = uid(id);
        asset.duration_seconds = Some(duration);
        self.project.assets.push(asset.clone());
        asset
    }

    fn add_gen_video(
        &mut self,
        id: u128,
        name: &str,
        version: &str,
        fps: f64,
        frames: u32,
    ) -> Asset {
        let folder = PathBuf::from(format!("generated/video/{}", uid(id)));
        let root = self.project.project_path.as_ref().unwrap().join(&folder);
        fs_create(&root);
        write_file(&root.join(format!("{version}.mp4")));
        let mut asset = Asset::new_generative_video(name, folder, fps, frames);
        asset.id = uid(id);
        if let AssetKind::GenerativeVideo { active_version, .. } = &mut asset.kind {
            *active_version = Some(version.to_string());
        }
        asset.duration_seconds = Some(frames as f64 / fps);
        self.project.assets.push(asset.clone());
        asset
    }

    fn place(
        &mut self,
        clip_id: u128,
        asset_id: Uuid,
        track_index: usize,
        start: f64,
        duration: f64,
    ) -> Clip {
        let clip = Clip {
            id: uid(clip_id),
            asset_id,
            track_id: self.project.tracks[track_index].id,
            start_time: start,
            duration,
            trim_in_seconds: 0.0,
            volume: 1.0,
            label: None,
            image_mode: ClipImageMode::Still,
            time_mode: ClipTimeMode::Crop,
            transform: Default::default(),
            bridge: None,
        };
        self.project.clips.push(clip.clone());
        clip
    }
}

fn fs_create(path: &std::path::Path) {
    std::fs::create_dir_all(path).expect("create dir");
}

fn write_file(path: &std::path::Path) {
    std::fs::write(path, b"media").expect("write file");
}

fn follow_auto() -> MediaBindingSpec {
    MediaBindingSpec::follow_auto()
}

#[test]
fn unpinned_legacy_reference_becomes_follow_auto() {
    let harness = Harness::new();
    let value = InputValue::AssetRef {
        asset_id: uid(1),
        source_clip_id: Some(uid(2)),
        pinned: false,
        frame_reference: Some(SourceFrameReference::First),
    };
    let spec = legacy_input_to_binding(
        &value,
        &image_field(Some(InputRole::StartImage)),
        &harness.project,
    )
    .unwrap();
    assert!(matches!(
        spec.source,
        MediaBindingSource::FollowTimeline { .. }
    ));
    assert!(matches!(
        spec.sample,
        MediaSample::Frame {
            at: MediaFramePoint::SourceStart
        }
    ));
}

#[test]
fn pinned_clip_becomes_locked_clip_with_version() {
    let mut harness = Harness::new();
    let gen = harness.add_gen_video(11, "Gen A", "v3", 24.0, 24);
    let clip = harness.place(12, gen.id, 1, 0.0, 1.0);
    let value = InputValue::AssetRef {
        asset_id: gen.id,
        source_clip_id: Some(clip.id),
        pinned: true,
        frame_reference: Some(SourceFrameReference::Last),
    };
    let spec = legacy_input_to_binding(
        &value,
        &image_field(Some(InputRole::StartImage)),
        &harness.project,
    )
    .unwrap();
    match spec.source {
        MediaBindingSource::TimelineClip { clip_id, version } => {
            assert_eq!(clip_id, clip.id);
            assert_eq!(version.as_deref(), Some("v3"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn pinned_asset_becomes_project_asset() {
    let mut harness = Harness::new();
    let image = harness.add_image(13, "Still", "images/still.png");
    let value = InputValue::AssetRef {
        asset_id: image.id,
        source_clip_id: None,
        pinned: true,
        frame_reference: None,
    };
    let spec = legacy_input_to_binding(&value, &image_field(None), &harness.project).unwrap();
    assert!(matches!(
        spec.source,
        MediaBindingSource::ProjectAsset { asset_id, version: None } if asset_id == image.id
    ));
}

#[test]
fn generation_ref_becomes_exact_version() {
    let harness = Harness::new();
    let value = InputValue::GenerationRef {
        asset_id: harness.target.id,
        version: "v9".into(),
        frame_reference: Some(SourceFrameReference::First),
    };
    let spec = legacy_input_to_binding(&value, &image_field(None), &harness.project).unwrap();
    match spec.source {
        MediaBindingSource::ProjectAsset { asset_id, version } => {
            assert_eq!(asset_id, harness.target.id);
            assert_eq!(version.as_deref(), Some("v9"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn canonical_binding_overrides_legacy_alias() {
    let mut harness = Harness::new();
    let image = harness.add_image(14, "A", "images/a.png");
    harness.config.inputs.insert(
        "start_image".into(),
        InputValue::AssetRef {
            asset_id: image.id,
            source_clip_id: None,
            pinned: true,
            frame_reference: None,
        },
    );
    harness
        .config
        .media_bindings
        .insert("start_image".into(), MediaBindingSpec::follow_auto());
    let field = image_field(Some(InputRole::StartImage));
    let spec = lookup_media_binding(&harness.config, &field, &harness.project).unwrap();
    assert!(matches!(
        spec.source,
        MediaBindingSource::FollowTimeline { .. }
    ));
}

#[test]
fn output_first_and_last_frames() {
    let harness = Harness::new();
    let field = image_field(Some(InputRole::StartImage));
    let ctx = harness.ctx(&field);
    let window = target_window(ctx, &harness.context).unwrap();
    assert!((window.first_global() - 5.0).abs() < 1e-9);
    assert!((window.last_global() - (5.0 + 23.0 / 24.0)).abs() < 1e-9);
}

#[test]
fn last_frame_falls_back_from_duration() {
    let mut harness = Harness::new();
    if let AssetKind::GenerativeVideo { frame_count, .. } = &mut harness.project.assets[0].kind {
        *frame_count = 0;
    }
    harness.target = harness.project.assets[0].clone();
    let field = image_field(Some(InputRole::EndImage));
    let ctx = harness.ctx(&field);
    let window = target_window(ctx, &harness.context).unwrap();
    let expected = (6.0_f64 - 1.0 / 24.0).max(5.0);
    assert!((window.last_global() - expected).abs() < 1e-9);
}

#[test]
fn touching_boundaries_ignore_float_drift() {
    assert!(boundaries_touch(1.0, 1.0 + 1e-10, 24.0));
    assert!(boundaries_touch(5.0, 4.999999999, 24.0));
    assert!(!boundaries_touch(5.0, 5.0 + 1.0 / 24.0, 24.0));
}

#[test]
fn exact_keyframe_beats_touching_previous() {
    let mut harness = Harness::new();
    let key = harness.add_image(21, "Key", "images/key.png");
    let prev = harness.add_gen_video(22, "Prev", "v1", 24.0, 24);
    harness.place(31, prev.id, 1, 4.0, 1.0);
    let mut key_clip = harness.place(32, key.id, 2, 5.0, 1.0);
    key_clip.image_mode = ClipImageMode::Keyframe;
    if let Some(clip) = harness
        .project
        .clips
        .iter_mut()
        .find(|clip| clip.id == key_clip.id)
    {
        clip.image_mode = ClipImageMode::Keyframe;
    }
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::ExactKeyframe));
    assert_eq!(plan.source_asset_id, Some(key.id));
}

#[test]
fn touching_previous_beats_covering_when_preferred() {
    let mut harness = Harness::new();
    let prev = harness.add_gen_video(23, "Prev", "v1", 24.0, 24);
    let below = harness.add_image(24, "Below", "images/below.png");
    harness.place(33, prev.id, 1, 4.0, 1.0);
    harness.place(34, below.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::TouchingPrevious));
    assert_eq!(plan.source_asset_id, Some(prev.id));
}

#[test]
fn covering_beats_touching_when_not_preferred() {
    let mut harness = Harness::new();
    let prev = harness.add_gen_video(25, "Prev", "v1", 24.0, 24);
    let below = harness.add_image(26, "Below", "images/below.png");
    harness.place(35, prev.id, 1, 4.0, 1.0);
    harness.place(36, below.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::FollowTimeline {
            query: TimelineSourceQuery {
                scope: TimelineTrackScope::Auto,
                prefer_touching: false,
            },
        },
        sample: MediaSample::Auto,
        coverage: MediaCoveragePolicy::Strict,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::CoveringFrame));
    assert_eq!(plan.source_asset_id, Some(below.id));
}

#[test]
fn same_track_continuation_uses_previous_final_frame() {
    let mut harness = Harness::new();
    let prev = harness.add_gen_video(27, "A", "v1", 24.0, 24);
    harness.place(37, prev.id, 1, 4.0, 1.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert_eq!(plan.relation, Some(MediaBindingRelation::TouchingPrevious));
    let expected = (1.0_f64 - 1.0 / 24.0).max(0.0);
    assert!((plan.source_frame_time.unwrap() - expected).abs() < 1e-6);
}

#[test]
fn touching_previous_still_image_on_same_track() {
    let mut harness = Harness::new();
    let still = harness.add_image(41, "StillPrev", "images/still-prev.png");
    harness.place(51, still.id, 1, 4.0, 1.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::TouchingPrevious));
    assert_eq!(plan.source_asset_id, Some(still.id));
}

#[test]
fn end_oriented_input_selects_touching_next() {
    let mut harness = Harness::new();
    let next = harness.add_gen_video(28, "Next", "v1", 24.0, 24);
    harness.place(38, next.id, 1, 6.0, 1.0);
    let field = image_field(Some(InputRole::EndImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::follow_auto(),
        sample: MediaSample::Frame {
            at: MediaFramePoint::OutputEnd,
        },
        coverage: MediaCoveragePolicy::Strict,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::TouchingNext));
    assert!((plan.source_frame_time.unwrap() - 0.0).abs() < 1e-9);
}

#[test]
fn immediate_below_beats_farther_below() {
    let mut harness = Harness::new();
    let near = harness.add_image(29, "Near", "images/near.png");
    let far = harness.add_image(30, "Far", "images/far.png");
    harness.place(39, near.id, 2, 4.5, 2.0);
    harness.place(40, far.id, 3, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert_eq!(plan.source_asset_id, Some(near.id));
}

#[test]
fn below_scope_excludes_same_and_above() {
    let mut harness = Harness::new();
    let same = harness.add_image(41, "Same", "images/same.png");
    let above = harness.add_image(42, "Above", "images/above.png");
    let below = harness.add_image(43, "Below", "images/below2.png");
    harness.place(51, same.id, 1, 4.5, 2.0);
    harness.place(52, above.id, 0, 4.5, 2.0);
    harness.place(53, below.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::FollowTimeline {
            query: TimelineSourceQuery {
                scope: TimelineTrackScope::Below,
                prefer_touching: true,
            },
        },
        ..MediaBindingSpec::follow_auto()
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert_eq!(plan.source_asset_id, Some(below.id));
}

#[test]
fn specific_track_uses_stable_id() {
    let mut harness = Harness::new();
    let a = harness.add_image(44, "A", "images/a2.png");
    let b = harness.add_image(45, "B", "images/b2.png");
    harness.place(54, a.id, 2, 4.5, 2.0);
    harness.place(55, b.id, 0, 4.5, 2.0);
    let track_id = harness.project.tracks[0].id;
    let field = image_field(Some(InputRole::StartImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::FollowTimeline {
            query: TimelineSourceQuery {
                scope: TimelineTrackScope::SpecificTrack { track_id },
                prefer_touching: true,
            },
        },
        ..MediaBindingSpec::follow_auto()
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert_eq!(plan.source_asset_id, Some(b.id));
}

#[test]
fn deterministic_uuid_tie_break() {
    let mut harness = Harness::new();
    let low = harness.add_image(1, "Low", "images/low.png");
    let high = harness.add_image(99, "High", "images/high.png");
    harness.place(2, high.id, 2, 4.5, 2.0);
    harness.place(3, low.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert_eq!(plan.source_clip_id, Some(uid(2).min(uid(3))));
}

#[test]
fn no_nearest_distant_boundary_fallback() {
    let mut harness = Harness::new();
    let distant = harness.add_image(46, "Far", "images/distant.png");
    harness.place(56, distant.id, 2, 0.0, 1.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(!plan.is_ok());
    assert!(matches!(
        plan.errors.first(),
        Some(MediaBindingError::NoTimelineCandidate { .. })
    ));
}

#[test]
fn target_asset_self_reference_excluded() {
    let mut harness = Harness::new();
    harness.place(57, harness.target.id, 1, 4.0, 1.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(!plan.is_ok());
}

#[test]
fn hollow_generative_candidate_excluded() {
    let mut harness = Harness::new();
    let mut hollow =
        Asset::new_generative_video("Hollow", PathBuf::from("generated/video/hollow"), 24.0, 24);
    hollow.id = uid(47);
    harness.project.assets.push(hollow.clone());
    harness.place(58, hollow.id, 1, 4.0, 1.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(!plan.is_ok());
}

#[test]
fn still_image_covers_interval() {
    let mut harness = Harness::new();
    let still = harness.add_image(48, "Still", "images/still2.png");
    harness.place(59, still.id, 2, 4.0, 3.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert_eq!(plan.relation, Some(MediaBindingRelation::CoveringFrame));
}

#[test]
fn keyframe_image_only_exact_frame() {
    let mut harness = Harness::new();
    let key = harness.add_image(49, "Key", "images/key2.png");
    let mut clip = harness.place(60, key.id, 2, 5.0 + 1.0 / 24.0, 1.0);
    if let Some(stored) = harness
        .project
        .clips
        .iter_mut()
        .find(|item| item.id == clip.id)
    {
        stored.image_mode = ClipImageMode::Keyframe;
        clip.image_mode = ClipImageMode::Keyframe;
    }
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(!plan.is_ok());
}

#[test]
fn video_covering_arbitrary_frame_maps_source_time() {
    let mut harness = Harness::new();
    let video = harness.add_video(50, "Plate", "video/plate.mp4", 10.0);
    let mut clip = harness.place(61, video.id, 2, 4.0, 4.0);
    if let Some(stored) = harness
        .project
        .clips
        .iter_mut()
        .find(|item| item.id == clip.id)
    {
        stored.trim_in_seconds = 1.0;
        clip.trim_in_seconds = 1.0;
    }
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::CoveringFrame));
    assert!((plan.source_frame_time.unwrap() - 2.0).abs() < 1e-6);
}

#[test]
fn audio_aligned_slice_maps_5_to_10() {
    let mut harness = Harness::new();
    harness.context.duration = 5.0;
    if let Some(clip) = harness
        .project
        .clips
        .iter_mut()
        .find(|clip| clip.id == harness.context.id)
    {
        clip.duration = 5.0;
    }
    let audio = harness.add_audio(70, "Dialogue", "audio/dialogue.wav", 10.0);
    harness.place(71, audio.id, 3, 0.0, 10.0);
    let field = audio_field();
    harness.provider = provider_with(vec![field.clone()]);
    let spec = MediaBindingSpec {
        source: MediaBindingSource::follow_auto(),
        sample: MediaSample::AlignedRange,
        coverage: MediaCoveragePolicy::Strict,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    let range = plan.source_range.unwrap();
    assert!((range.start_seconds - 5.0).abs() < 1e-6);
    assert!((range.end_seconds - 10.0).abs() < 1e-6);
}

#[test]
fn partial_audio_range_fails_strict() {
    let mut harness = Harness::new();
    harness.context.duration = 5.0;
    if let Some(clip) = harness
        .project
        .clips
        .iter_mut()
        .find(|clip| clip.id == harness.context.id)
    {
        clip.duration = 5.0;
    }
    let audio = harness.add_audio(72, "Dialogue", "audio/short.wav", 9.0);
    harness.place(73, audio.id, 3, 0.0, 9.0);
    let field = audio_field();
    harness.provider = provider_with(vec![field.clone()]);
    let spec = MediaBindingSpec {
        source: MediaBindingSource::follow_auto(),
        sample: MediaSample::AlignedRange,
        coverage: MediaCoveragePolicy::Strict,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(!plan.is_ok());
    let message = plan.primary_error_message().unwrap();
    assert!(message.contains("1.000 s") || message.contains("missing"));
}

#[test]
fn crop_mapping_honors_trim_in() {
    let mut harness = Harness::new();
    let video = harness.add_video(74, "Crop", "video/crop.mp4", 12.0);
    let clip = harness.place(75, video.id, 2, 5.0, 1.0);
    if let Some(stored) = harness
        .project
        .clips
        .iter_mut()
        .find(|item| item.id == clip.id)
    {
        stored.trim_in_seconds = 2.5;
        stored.time_mode = ClipTimeMode::Crop;
    }
    let field = image_field(Some(InputRole::StartImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::TimelineClip {
            clip_id: clip.id,
            version: None,
        },
        sample: MediaSample::Frame {
            at: MediaFramePoint::OutputStart,
        },
        coverage: MediaCoveragePolicy::Strict,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert!((plan.source_frame_time.unwrap() - 2.5).abs() < 1e-6);
}

#[test]
fn stretch_mapping_produces_source_range_and_retime() {
    let mut harness = Harness::new();
    harness.context.duration = 2.0;
    if let Some(clip) = harness
        .project
        .clips
        .iter_mut()
        .find(|clip| clip.id == harness.context.id)
    {
        clip.duration = 2.0;
    }
    let video = harness.add_video(76, "Stretch", "video/stretch.mp4", 4.0);
    let clip = harness.place(77, video.id, 2, 5.0, 2.0);
    if let Some(stored) = harness
        .project
        .clips
        .iter_mut()
        .find(|item| item.id == clip.id)
    {
        stored.time_mode = ClipTimeMode::Stretch;
        stored.trim_in_seconds = 0.0;
    }
    let field = video_field();
    harness.provider = provider_with(vec![field.clone()]);
    let spec = MediaBindingSpec {
        source: MediaBindingSource::TimelineClip {
            clip_id: clip.id,
            version: None,
        },
        sample: MediaSample::AlignedRange,
        coverage: MediaCoveragePolicy::Strict,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    let range = plan.source_range.unwrap();
    assert!((range.start_seconds - 0.0).abs() < 1e-6);
    assert!((range.end_seconds - 4.0).abs() < 1e-6);
    assert_eq!(plan.retime_to_duration, Some(2.0));
}

#[test]
fn locked_source_ignores_new_competing_candidate() {
    let mut harness = Harness::new();
    let first = harness.add_image(78, "First", "images/first.png");
    let second = harness.add_image(79, "Second", "images/second.png");
    let clip = harness.place(80, first.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let locked = MediaBindingSpec {
        source: MediaBindingSource::TimelineClip {
            clip_id: clip.id,
            version: None,
        },
        sample: MediaSample::Frame {
            at: MediaFramePoint::OutputStart,
        },
        coverage: MediaCoveragePolicy::Strict,
    };
    harness.place(81, second.id, 1, 4.0, 1.0);
    let plan = resolve_media_binding(harness.ctx(&field), &locked);
    assert_eq!(plan.source_asset_id, Some(first.id));
    assert_eq!(plan.relation, Some(MediaBindingRelation::ExplicitClip));
}

#[test]
fn follow_changes_after_higher_ranked_candidate_added() {
    let mut harness = Harness::new();
    let covering = harness.add_image(82, "Cover", "images/cover.png");
    harness.place(83, covering.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let first = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert_eq!(first.source_asset_id, Some(covering.id));
    let key = harness.add_image(84, "Key", "images/key3.png");
    let mut clip = harness.place(85, key.id, 0, 5.0, 1.0);
    if let Some(stored) = harness
        .project
        .clips
        .iter_mut()
        .find(|item| item.id == clip.id)
    {
        stored.image_mode = ClipImageMode::Keyframe;
        clip.image_mode = ClipImageMode::Keyframe;
    }
    let second = resolve_media_binding(harness.ctx(&field), &follow_auto());
    assert_eq!(second.relation, Some(MediaBindingRelation::ExactKeyframe));
    assert_eq!(second.source_asset_id, Some(key.id));
}

#[test]
fn frozen_input_remains_valid_after_source_deletion() {
    let mut harness = Harness::new();
    let image = harness.add_image(86, "FreezeMe", "images/freeze.png");
    let frozen_rel =
        PathBuf::from("generated/video/target/inputs/frozen/start_image/start_image.png");
    let frozen_abs = harness
        .project
        .project_path
        .as_ref()
        .unwrap()
        .join(&frozen_rel);
    fs_create(frozen_abs.parent().unwrap());
    write_file(&frozen_abs);
    let spec = MediaBindingSpec {
        source: MediaBindingSource::FrozenArtifact {
            path: frozen_rel,
            media_type: BoundMediaType::Image,
            original_binding: Some(Box::new(MediaBindingSpec {
                source: MediaBindingSource::ProjectAsset {
                    asset_id: image.id,
                    version: None,
                },
                sample: MediaSample::Frame {
                    at: MediaFramePoint::SourceStart,
                },
                coverage: MediaCoveragePolicy::Strict,
            })),
            origin: None,
        },
        sample: MediaSample::Frame {
            at: MediaFramePoint::OutputStart,
        },
        coverage: MediaCoveragePolicy::Strict,
    };
    harness.project.assets.retain(|asset| asset.id != image.id);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::Frozen));
}

#[test]
fn lock_source_transition_from_follow() {
    let mut harness = Harness::new();
    let image = harness.add_image(87, "Lock", "images/lock.png");
    let clip = harness.place(88, image.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    let locked = lock_source_spec(&plan, &follow_auto()).unwrap();
    match locked.source {
        MediaBindingSource::TimelineClip { clip_id, .. } => assert_eq!(clip_id, clip.id),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unfreeze_restores_original_binding() {
    let original = MediaBindingSpec::follow_auto();
    let frozen = MediaBindingSpec {
        source: MediaBindingSource::FrozenArtifact {
            path: PathBuf::from("generated/x/inputs/frozen/a.png"),
            media_type: BoundMediaType::Image,
            original_binding: Some(Box::new(original.clone())),
            origin: None,
        },
        sample: MediaSample::Frame {
            at: MediaFramePoint::OutputStart,
        },
        coverage: MediaCoveragePolicy::Strict,
    };
    assert_eq!(unfreeze_spec(&frozen).unwrap(), original);
}

#[test]
fn follow_requires_context() {
    let harness = Harness::new();
    let field = image_field(Some(InputRole::StartImage));
    let ctx = MediaResolveContext {
        context_clip_id: None,
        ..harness.ctx(&field)
    };
    let plan = resolve_media_binding(ctx, &follow_auto());
    assert!(matches!(
        plan.errors.first(),
        Some(MediaBindingError::ContextRequired)
    ));
}

#[test]
fn multiple_placements_require_context() {
    let mut harness = Harness::new();
    harness.place(90, harness.target.id, 0, 8.0, 1.0);
    let error =
        resolve_generation_context(&harness.project, harness.target.id, None, None).unwrap_err();
    assert!(matches!(
        error,
        MediaBindingError::MultiplePlacementsRequireContext { count: 2 }
    ));
}

#[test]
fn explicit_asset_works_without_timeline() {
    let mut harness = Harness::new();
    let image = harness.add_image(91, "Solo", "images/solo.png");
    let field = image_field(Some(InputRole::StartImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::ProjectAsset {
            asset_id: image.id,
            version: None,
        },
        sample: MediaSample::Frame {
            at: MediaFramePoint::SourceStart,
        },
        coverage: MediaCoveragePolicy::Strict,
    };
    let ctx = MediaResolveContext {
        context_clip_id: None,
        ..harness.ctx(&field)
    };
    let plan = resolve_media_binding(ctx, &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    assert_eq!(plan.relation, Some(MediaBindingRelation::ExplicitAsset));
}

#[test]
fn unsupported_coverage_policy_errors() {
    let harness = Harness::new();
    let field = image_field(Some(InputRole::StartImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::follow_auto(),
        sample: MediaSample::Auto,
        coverage: MediaCoveragePolicy::Loop,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(matches!(
        plan.errors.first(),
        Some(MediaBindingError::UnsupportedCoveragePolicy(_))
    ));
}

#[test]
fn output_offset_and_frame_index() {
    let mut harness = Harness::new();
    let image = harness.add_image(92, "Off", "images/off.png");
    harness.place(93, image.id, 2, 4.0, 3.0);
    let field = image_field(Some(InputRole::StartImage));
    let spec = MediaBindingSpec {
        source: MediaBindingSource::follow_auto(),
        sample: MediaSample::Frame {
            at: MediaFramePoint::OutputFrame { frame: 12 },
        },
        coverage: MediaCoveragePolicy::Strict,
    };
    let plan = resolve_media_binding(harness.ctx(&field), &spec);
    assert!(plan.is_ok(), "{:?}", plan.errors);
    let expected = 5.0 + 12.0 / 24.0;
    assert!((plan.target_frame_time.unwrap() - expected).abs() < 1e-6);
}

#[test]
fn config_locked_version_is_found() {
    let harness = Harness::new();
    let mut config = GenerativeConfig::default();
    config.media_bindings.insert(
        "start_image".into(),
        MediaBindingSpec {
            source: MediaBindingSource::ProjectAsset {
                asset_id: harness.target.id,
                version: Some("v1".into()),
            },
            sample: MediaSample::Auto,
            coverage: MediaCoveragePolicy::Strict,
        },
    );
    assert!(config_references_locked_version(
        &config,
        &harness.project,
        harness.target.id,
        "v1"
    ));
    assert!(!config_references_locked_version(
        &config,
        &harness.project,
        harness.target.id,
        "v2"
    ));
}

#[test]
fn inspect_config_reports_resolved_follow() {
    let mut harness = Harness::new();
    let image = harness.add_image(110, "Inspect", "images/inspect.png");
    harness.place(111, image.id, 2, 4.5, 2.0);
    harness
        .config
        .media_bindings
        .insert("start_image".into(), follow_auto());
    let map = inspect_config_media_bindings(
        &harness.project,
        harness.target.id,
        Some(harness.context.id),
        &harness.provider,
        &harness.config,
    );
    let json = map.get("start_image").expect("start_image inspection");
    assert_eq!(json["ok"], true);
    let summary = json["summary"].as_str().unwrap();
    assert!(summary.contains("Resolved now"), "{summary}");
    assert!(summary.contains("Inspect"), "{summary}");
}

#[test]
fn locked_snapshot_ignores_later_higher_ranked_candidate() {
    let mut harness = Harness::new();
    let image = harness.add_image(112, "Snap", "images/snap.png");
    let clip = harness.place(113, image.id, 2, 4.5, 2.0);
    let field = image_field(Some(InputRole::StartImage));
    let plan = resolve_media_binding(harness.ctx(&field), &follow_auto());
    let snapshot = lock_source_spec(&plan, &follow_auto()).unwrap();
    let key = harness.add_image(114, "KeyWin", "images/keywin.png");
    let key_clip = harness.place(115, key.id, 0, 5.0, 1.0);
    if let Some(stored) = harness
        .project
        .clips
        .iter_mut()
        .find(|item| item.id == key_clip.id)
    {
        stored.image_mode = ClipImageMode::Keyframe;
    }
    let live = resolve_media_binding(harness.ctx(&field), &follow_auto());
    let locked = resolve_media_binding(harness.ctx(&field), &snapshot);
    assert_eq!(live.relation, Some(MediaBindingRelation::ExactKeyframe));
    assert_eq!(locked.source_clip_id, Some(clip.id));
    assert_eq!(locked.relation, Some(MediaBindingRelation::ExplicitClip));
}
