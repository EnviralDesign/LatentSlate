//! Asset Lab authored documents and immutable submission snapshots.

use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{BatchSettings, InputValue, MediaBindingSpec, ReferenceSizing, ResolvedMediaInput};

fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetLabMaskGeometry {
    pub input_field: String,
    pub width: u32,
    pub height: u32,
    pub base: ResolvedMediaInput,
    pub sizing: ReferenceSizing,
    pub source_identity: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetLabMask {
    /// Project-relative lossless coverage image: zero is unpainted, 255 is painted.
    pub path: PathBuf,
    pub geometry: AssetLabMaskGeometry,
    pub has_content: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetLabRegion {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub text: Option<String>,
    /// Fractions of the independently sized canvas axes (x, y, width, height).
    pub bounds: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetLabAuthoring {
    pub initialized: bool,
    pub working_version: Option<String>,
    pub mask: Option<AssetLabMask>,
    #[serde(default = "enabled")]
    pub mask_enabled: bool,
    pub regions: Vec<AssetLabRegion>,
    #[serde(default = "enabled")]
    pub regions_enabled: bool,
}

impl Default for AssetLabAuthoring {
    fn default() -> Self {
        Self {
            initialized: false,
            working_version: None,
            mask: None,
            mask_enabled: true,
            regions: Vec::new(),
            regions_enabled: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetLabSnapshot {
    pub provider_id: Option<Uuid>,
    pub inputs: HashMap<String, InputValue>,
    #[serde(default)]
    pub reference_slots: HashMap<String, InputValue>,
    pub media_bindings: HashMap<String, MediaBindingSpec>,
    pub reference_sizing: HashMap<String, ReferenceSizing>,
    pub batch: BatchSettings,
    pub authoring: AssetLabAuthoring,
}

impl AssetLabSnapshot {
    pub fn from_config(config: &super::GenerativeConfig) -> Self {
        Self {
            provider_id: config.provider_id,
            inputs: config.inputs.clone(),
            reference_slots: config.reference_slots.clone(),
            media_bindings: config.media_bindings.clone(),
            reference_sizing: config.reference_sizing.clone(),
            batch: config.batch.clone(),
            authoring: config.lab_authoring.clone(),
        }
    }

    pub fn apply(&self, config: &mut super::GenerativeConfig) {
        config.provider_id = self.provider_id;
        config.inputs = self.inputs.clone();
        config.reference_slots = self.reference_slots.clone();
        config.media_bindings = self.media_bindings.clone();
        config.reference_sizing = self.reference_sizing.clone();
        config.batch = self.batch.clone();
        config.lab_authoring = self.authoring.clone();
    }
}

/// Queue correlation only; never persisted as an authoring history or batch group.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetLabSubmission {
    pub session_id: Uuid,
    pub revision: u64,
    pub parent_node_id: Option<Uuid>,
    pub allow_advance: bool,
}

/// UI profiles describe authoring affordances, never inference capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetLabAuthoringProfile {
    Generic,
    Mask,
    Regions,
}

pub fn asset_lab_authoring_profile(provider: &super::ProviderEntry) -> AssetLabAuthoringProfile {
    match &provider.connection {
        super::ProviderConnection::LatentSlateEngine { tool_key, .. } => match tool_key.as_str() {
            "qwen2511.edit" => AssetLabAuthoringProfile::Mask,
            "ideogram4.text_to_image" => AssetLabAuthoringProfile::Regions,
            _ => AssetLabAuthoringProfile::Generic,
        },
        _ => AssetLabAuthoringProfile::Generic,
    }
}

pub fn asset_lab_submission_blocker(
    config: &super::GenerativeConfig,
    provider: &super::ProviderEntry,
) -> Option<&'static str> {
    match asset_lab_authoring_profile(provider) {
        AssetLabAuthoringProfile::Mask if config.lab_authoring.mask_enabled
            && config.lab_authoring.mask.as_ref().is_some_and(|mask| mask.has_content) =>
            Some("Masked generation is not connected yet. Turn off Use mask in Asset Lab to generate a full-image edit."),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_lab_v4_legacy_config_and_authored_snapshot_are_independent() {
        let mut config: super::super::GenerativeConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.lab_authoring.initialized);
        config.lab_authoring.regions.push(AssetLabRegion {
            id: Uuid::new_v4(),
            name: "Object".into(),
            description: "Before".into(),
            text: None,
            bounds: [0.1, 0.2, 0.3, 0.4],
        });
        let snapshot = AssetLabSnapshot::from_config(&config);
        config.lab_authoring.regions[0].description = "After".into();
        assert_eq!(snapshot.authoring.regions[0].description, "Before");
        let saved = serde_json::to_string(&snapshot).unwrap();
        let loaded: AssetLabSnapshot = serde_json::from_str(&saved).unwrap();
        loaded.apply(&mut config);
        assert_eq!(config.lab_authoring.regions[0].description, "Before");
    }

    #[test]
    fn asset_lab_v4_region_effect_can_be_disabled_without_losing_layout() {
        let provider = super::super::ProviderEntry::new(
            "An arbitrary display name",
            super::super::ProviderOutputType::Image,
            super::super::ProviderConnection::LatentSlateEngine {
                base_url: "http://unused".into(),
                api_key: None,
                tool_key: "ideogram4.text_to_image".into(),
                schema_revision: 1,
                schema_hash: "test".into(),
                recipe: None,
                available: true,
                unavailable_reason: None,
            },
        );
        let mut config = super::super::GenerativeConfig::default();
        config.lab_authoring.regions.push(AssetLabRegion {
            id: Uuid::new_v4(),
            name: "Text".into(),
            description: String::new(),
            text: Some("Hello".into()),
            bounds: [0.0, 0.0, 1.0, 1.0],
        });
        assert!(asset_lab_submission_blocker(&config, &provider).is_none());
        config.lab_authoring.regions_enabled = false;
        assert!(asset_lab_submission_blocker(&config, &provider).is_none());
        assert_eq!(config.lab_authoring.regions.len(), 1);
    }
}
