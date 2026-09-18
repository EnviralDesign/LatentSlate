//! Deterministic Ideogram v4 structured-caption serialization for Asset Lab regions.
//!
//! Engine encodes the `prompt` string without rewriting it. When Ideogram regions are
//! enabled, LatentSlate submits the official caption JSON through that existing field.
//! UI ids and names stay out of the caption unless the schema names them.

use serde_json::Value;

use crate::state::{
    AssetLabAuthoring, AssetLabRegion, GenerativeConfig, InputValue, ProviderConnection,
    ProviderEntry, ProviderInputType,
};

const GRID: f32 = 1000.0;
const PROMPT_FIELD: &str = "prompt";

/// `[y_min, x_min, y_max, x_max]` on the official 0–1000 grid.
pub fn grid_bbox(bounds: [f32; 4]) -> [i32; 4] {
    let [x, y, width, height] = bounds;
    let mut x_min = to_grid(x);
    let mut y_min = to_grid(y);
    let mut x_max = to_grid(x + width);
    let mut y_max = to_grid(y + height);
    if y_min > y_max {
        std::mem::swap(&mut y_min, &mut y_max);
    }
    if x_min > x_max {
        std::mem::swap(&mut x_min, &mut x_max);
    }
    [y_min, x_min, y_max, x_max]
}

pub fn is_ideogram4_text_to_image(provider: &ProviderEntry) -> bool {
    matches!(
        &provider.connection,
        ProviderConnection::LatentSlateEngine { tool_key, .. }
            if tool_key == "ideogram4.text_to_image"
    )
}

/// Serialize enabled regions into an official caption. `None` keeps the ordinary prompt.
pub fn structured_caption(scene_prompt: &str, authoring: &AssetLabAuthoring) -> Option<String> {
    if !authoring.regions_enabled || authoring.regions.is_empty() {
        return None;
    }
    Some(serialize_caption(scene_prompt, &authoring.regions))
}

pub fn apply_submitted_prompt(
    provider: &ProviderEntry,
    config: &GenerativeConfig,
    values: &mut std::collections::HashMap<String, Value>,
    snapshot: &mut std::collections::HashMap<String, InputValue>,
) {
    if !is_ideogram4_text_to_image(provider) {
        return;
    }
    if !provider
        .inputs
        .iter()
        .any(|input| input.name == PROMPT_FIELD && input.input_type == ProviderInputType::Text)
    {
        return;
    }
    let scene = values
        .get(PROMPT_FIELD)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let Some(caption) = structured_caption(&scene, &config.lab_authoring) else {
        return;
    };
    let value = Value::String(caption);
    values.insert(PROMPT_FIELD.to_string(), value.clone());
    snapshot.insert(PROMPT_FIELD.to_string(), InputValue::Literal { value });
}

fn serialize_caption(scene_prompt: &str, regions: &[AssetLabRegion]) -> String {
    let scene = scene_prompt.trim();
    let mut root = Vec::new();
    if !scene.is_empty() {
        root.push(("high_level_description", json_string(scene)));
    }
    let elements = regions
        .iter()
        .map(serialize_element)
        .collect::<Vec<_>>()
        .join(",");
    let deconstruction = json_object(&[
        ("background", json_string(scene)),
        ("elements", format!("[{elements}]")),
    ]);
    root.push(("compositional_deconstruction", deconstruction));
    json_object(&root)
}

fn serialize_element(region: &AssetLabRegion) -> String {
    let bbox = grid_bbox(region.bounds);
    let bbox_json = format!("[{},{},{},{}]", bbox[0], bbox[1], bbox[2], bbox[3]);
    let mut fields = vec![
        (
            "type",
            json_string(if region.text.is_some() { "text" } else { "obj" }),
        ),
        ("bbox", bbox_json),
    ];
    if let Some(text) = &region.text {
        fields.push(("text", json_string(text)));
    }
    fields.push(("desc", json_string(&region.description)));
    json_object(&fields)
}

fn json_object(fields: &[(&str, String)]) -> String {
    let mut out = String::from("{");
    for (index, (key, value)) in fields.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(key));
        out.push(':');
        out.push_str(value);
    }
    out.push('}');
    out
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn to_grid(value: f32) -> i32 {
    let scaled = (value * GRID).round();
    if !scaled.is_finite() {
        return 0;
    }
    scaled.clamp(0.0, GRID) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::generation::resolve_provider_inputs;
    use crate::state::{
        AssetLabRegion, CanvasContract, GenerativeConfig, InputRole, InputUi, ProviderConnection,
        ProviderEntry, ProviderInputField, ProviderOutputType,
    };
    use serde_json::json;
    use uuid::Uuid;

    fn region(
        name: &str,
        description: &str,
        text: Option<&str>,
        bounds: [f32; 4],
    ) -> AssetLabRegion {
        AssetLabRegion {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            text: text.map(str::to_string),
            bounds,
        }
    }

    fn ideogram_provider() -> ProviderEntry {
        let mut provider = ProviderEntry::new(
            "Ideogram v4 Text to Image",
            ProviderOutputType::Image,
            ProviderConnection::LatentSlateEngine {
                base_url: "http://127.0.0.1:8765".into(),
                api_key: None,
                tool_key: "ideogram4.text_to_image".into(),
                schema_revision: 1,
                schema_hash: "test".into(),
                recipe: None,
                available: true,
                unavailable_reason: None,
            },
        );
        provider.inputs = vec![
            ProviderInputField {
                ordered_collection: false,
                image_dimensions: None,
                paired_video_input: None,
                prompt_reference_token: None,
                name: "prompt".into(),
                label: "Prompt".into(),
                description: None,
                input_type: ProviderInputType::Text,
                required: true,
                default: Some(json!("")),
                role: None,
                ui: None,
            },
            ProviderInputField {
                ordered_collection: false,
                image_dimensions: None,
                paired_video_input: None,
                prompt_reference_token: None,
                name: "width".into(),
                label: "Width".into(),
                description: None,
                input_type: crate::state::ProviderInputType::Integer,
                required: true,
                default: Some(json!(1024)),
                role: Some(InputRole::Width),
                ui: Some(InputUi {
                    min: Some(256.0),
                    max: Some(2048.0),
                    step: Some(16.0),
                    ..InputUi::default()
                }),
            },
            ProviderInputField {
                ordered_collection: false,
                image_dimensions: None,
                paired_video_input: None,
                prompt_reference_token: None,
                name: "height".into(),
                label: "Height".into(),
                description: None,
                input_type: crate::state::ProviderInputType::Integer,
                required: true,
                default: Some(json!(1024)),
                role: Some(InputRole::Height),
                ui: Some(InputUi {
                    min: Some(256.0),
                    max: Some(2048.0),
                    step: Some(16.0),
                    ..InputUi::default()
                }),
            },
            ProviderInputField {
                ordered_collection: false,
                image_dimensions: None,
                paired_video_input: None,
                prompt_reference_token: None,
                name: "seed".into(),
                label: "Seed".into(),
                description: None,
                input_type: crate::state::ProviderInputType::Integer,
                required: true,
                default: Some(json!(0)),
                role: Some(InputRole::Seed),
                ui: Some(InputUi {
                    advanced: true,
                    ..InputUi::default()
                }),
            },
        ];
        provider.canvas = Some(CanvasContract {
            fixed_width: None,
            fixed_height: None,
            alignment: 16,
            min_side: 256,
            max_side: None,
            max_pixels: Some(1376 * 768),
            max_aspect: Some(4.0),
        });
        provider
    }

    #[test]
    fn grid_bbox_converts_axis_normalized_bounds_including_non_square_canvas() {
        assert_eq!(grid_bbox([0.2, 0.1, 0.3, 0.4]), [100, 200, 500, 500]);
        assert_eq!(grid_bbox([0.0, 0.0, 1.0, 1.0]), [0, 0, 1000, 1000]);
        assert_eq!(grid_bbox([-0.1, 1.2, 1.4, 0.3]), [1000, 0, 1000, 1000]);
        assert_eq!(grid_bbox([0.2006, 0.0, 0.1, 0.1]), [0, 201, 100, 301]);
        // Fractions are independent of pixel aspect; 1376×768 uses the same 0–1000 grid.
        assert_eq!(grid_bbox([0.05, 0.4, 0.35, 0.4]), [400, 50, 800, 400]);
    }

    #[test]
    fn structured_caption_matches_official_key_order_and_keeps_ui_names_out() {
        let mut authoring = AssetLabAuthoring {
            regions_enabled: true,
            regions: vec![
                region(
                    "Bench",
                    "a red bench on the left",
                    None,
                    [0.05, 0.4, 0.35, 0.4],
                ),
                region(
                    "Sign",
                    "white sans-serif sign in the upper right",
                    Some("PARK"),
                    [0.7, 0.05, 0.25, 0.15],
                ),
            ],
            ..AssetLabAuthoring::default()
        };
        let caption = structured_caption("A sunny park with a pond.", &authoring).unwrap();
        assert_eq!(
            caption,
            r#"{"high_level_description":"A sunny park with a pond.","compositional_deconstruction":{"background":"A sunny park with a pond.","elements":[{"type":"obj","bbox":[400,50,800,400],"desc":"a red bench on the left"},{"type":"text","bbox":[50,700,200,950],"text":"PARK","desc":"white sans-serif sign in the upper right"}]}}"#
        );
        assert!(!caption.contains("Bench"));
        assert!(!caption.contains("Sign"));
        authoring.regions_enabled = false;
        assert!(structured_caption("A sunny park with a pond.", &authoring).is_none());
        authoring.regions_enabled = true;
        authoring.regions.clear();
        assert!(structured_caption("A sunny park with a pond.", &authoring).is_none());
    }

    #[test]
    fn structured_caption_preserves_unicode_and_escapes_controls() {
        let authoring = AssetLabAuthoring {
            regions_enabled: true,
            regions: vec![region(
                "Quote",
                "a \"café\" sign",
                Some("café\nOK"),
                [0.0, 0.0, 0.2, 0.2],
            )],
            ..AssetLabAuthoring::default()
        };
        let caption = structured_caption("Scene \"note\"", &authoring).unwrap();
        assert!(caption.contains(r#""high_level_description":"Scene \"note\""#));
        assert!(caption.contains("café"));
        assert!(!caption.contains("\\u00e9"));
        assert!(caption.contains(r#""text":"café\nOK""#));
        serde_json::from_str::<Value>(&caption).unwrap();
    }

    #[test]
    fn resolve_submits_caption_from_enabled_regions_and_plain_prompt_when_disabled() {
        let project = crate::state::Project::new("ideogram-caption");
        let provider = ideogram_provider();
        let mut config = GenerativeConfig::default();
        config.inputs.insert(
            "prompt".into(),
            InputValue::Literal {
                value: json!("A sunny park with a pond."),
            },
        );
        config
            .inputs
            .insert("width".into(), InputValue::Literal { value: json!(1376) });
        config
            .inputs
            .insert("height".into(), InputValue::Literal { value: json!(768) });
        config.lab_authoring.regions = vec![
            region(
                "Bench",
                "a red bench on the left",
                None,
                [0.05, 0.4, 0.35, 0.4],
            ),
            region(
                "Sign",
                "white sans-serif sign in the upper right",
                Some("PARK"),
                [0.7, 0.05, 0.25, 0.15],
            ),
        ];
        let resolved = resolve_provider_inputs(&project, None, None, &provider, &config);
        assert!(
            resolved.input_errors.is_empty(),
            "{:?}",
            resolved.input_errors
        );
        assert_eq!(
            resolved.values["prompt"].as_str().unwrap(),
            r#"{"high_level_description":"A sunny park with a pond.","compositional_deconstruction":{"background":"A sunny park with a pond.","elements":[{"type":"obj","bbox":[400,50,800,400],"desc":"a red bench on the left"},{"type":"text","bbox":[50,700,200,950],"text":"PARK","desc":"white sans-serif sign in the upper right"}]}}"#
        );
        config.lab_authoring.regions.push(region(
            "Unused",
            "should stay saved",
            None,
            [0.8, 0.8, 0.1, 0.1],
        ));
        config.lab_authoring.regions_enabled = false;
        let resolved = resolve_provider_inputs(&project, None, None, &provider, &config);
        assert_eq!(
            resolved.values["prompt"].as_str().unwrap(),
            "A sunny park with a pond."
        );
        assert_eq!(config.lab_authoring.regions.len(), 3);
    }
}
