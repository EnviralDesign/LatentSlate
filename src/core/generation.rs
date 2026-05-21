#![allow(dead_code)]
// Provider input resolution is intentionally staged while the egui migration
// rebuilds the generation/attributes surface. Do not delete this without also
// replacing provider execution and seed handling.

use std::collections::HashMap;

use serde_json::Value;
use uuid::Uuid;

use crate::state::{
    GenerativeConfig, InputValue, ProviderEntry, ProviderInputField, ProviderInputType,
};

#[derive(Debug, Clone)]
pub struct ResolvedInputs {
    pub values: HashMap<String, Value>,
    pub snapshot: HashMap<String, InputValue>,
    pub missing_required: Vec<String>,
}

pub fn resolve_provider_inputs(
    provider: &ProviderEntry,
    config: &GenerativeConfig,
) -> ResolvedInputs {
    let mut values = HashMap::new();
    let mut snapshot = HashMap::new();
    let mut missing_required = Vec::new();

    for input in provider.inputs.iter() {
        let value = literal_input_value(config, &input.name).or_else(|| input.default.clone());

        if let Some(value) = value {
            values.insert(input.name.clone(), value.clone());
            snapshot.insert(input.name.clone(), InputValue::Literal { value });
        } else if input.required {
            missing_required.push(input.name.clone());
        }
    }

    ResolvedInputs {
        values,
        snapshot,
        missing_required,
    }
}

pub fn next_version_label(config: &GenerativeConfig) -> String {
    let mut max_version = 0u32;
    for record in config.versions.iter() {
        if let Some(number) = parse_version_number(&record.version) {
            max_version = max_version.max(number);
        }
    }
    if let Some(active) = config.active_version.as_ref() {
        if let Some(number) = parse_version_number(active) {
            max_version = max_version.max(number);
        }
    }
    format!("v{}", max_version + 1)
}

fn literal_input_value(config: &GenerativeConfig, name: &str) -> Option<Value> {
    config.inputs.get(name).and_then(|input| match input {
        InputValue::Literal { value } => Some(value.clone()),
        _ => None,
    })
}

fn parse_version_number(version: &str) -> Option<u32> {
    let trimmed = version.trim();
    let numeric = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))?;
    numeric.parse::<u32>().ok()
}

/// Resolve which provider input should be treated as the seed for batching.
pub fn resolve_seed_field(provider: &ProviderEntry, preferred: Option<&str>) -> Option<String> {
    if let Some(preferred) = preferred {
        if provider
            .inputs
            .iter()
            .any(|input| input.name == preferred && is_seed_candidate(input))
        {
            return Some(preferred.to_string());
        }
    }

    provider
        .inputs
        .iter()
        .find(|input| is_seed_candidate(input) && seed_like(&input.name, &input.label))
        .map(|input| input.name.clone())
}

/// Clone inputs and snapshot, overriding the seed field with a new value.
pub fn update_seed_inputs(
    values: &HashMap<String, Value>,
    snapshot: &HashMap<String, InputValue>,
    seed_field: &str,
    seed: i64,
) -> (HashMap<String, Value>, HashMap<String, InputValue>) {
    let mut values = values.clone();
    let mut snapshot = snapshot.clone();
    let seed_value = Value::Number(seed.into());
    values.insert(seed_field.to_string(), seed_value.clone());
    snapshot.insert(
        seed_field.to_string(),
        InputValue::Literal { value: seed_value },
    );
    (values, snapshot)
}

/// Generate a random seed suitable for numeric seed inputs.
pub fn random_seed_i64() -> i64 {
    let raw = Uuid::new_v4().as_u128();
    (raw % i64::MAX as u128) as i64
}

fn seed_like(name: &str, label: &str) -> bool {
    name.to_ascii_lowercase().contains("seed") || label.to_ascii_lowercase().contains("seed")
}

fn is_seed_candidate(input: &ProviderInputField) -> bool {
    matches!(
        input.input_type,
        ProviderInputType::Integer | ProviderInputType::Number
    )
}
