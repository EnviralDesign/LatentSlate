use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, Ui, Vec2};
use uuid::Uuid;

use crate::state::{
    ClipImageMode, ComfyOutputSelector, ComfyWorkflowRef, InputBinding, InputUi, ManifestInput,
    NodeSelector, ProviderConnection, ProviderEntry, ProviderInputField, ProviderInputType,
    ProviderManifest, ProviderOutputType, ProviderWorkflowKind, InputRole,
};
use crate::ui_kit as kit;

use super::{
    automation_checkbox, automation_selectable_value, inspector_numeric_field,
    inspector_numeric_rect, paint_truncated_row_text_bottom, paint_truncated_row_text_top,
    path_label, INSPECTOR_NUMERIC_H,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderTemplateKind {
    ComfyUi,
    OpenAiImage,
    XaiImage,
    XaiVideo,
}

impl Default for ProviderTemplateKind {
    fn default() -> Self {
        ProviderTemplateKind::ComfyUi
    }
}

impl ProviderTemplateKind {
    pub(super) const ALL: [ProviderTemplateKind; 4] = [
        ProviderTemplateKind::ComfyUi,
        ProviderTemplateKind::OpenAiImage,
        ProviderTemplateKind::XaiImage,
        ProviderTemplateKind::XaiVideo,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderBuilderTab {
    Output,
    Inputs,
}

#[derive(Clone, Debug)]
pub(super) struct ProviderBuilderState {
    pub(super) source_path: Option<PathBuf>,
    pub(super) provider_id: Uuid,
    pub(super) provider_name: String,
    pub(super) output_type: ProviderOutputType,
    pub(super) workflow_kind: ProviderWorkflowKind,
    pub(super) base_url: String,
    pub(super) workflow_path: Option<PathBuf>,
    pub(super) manifest_path: Option<PathBuf>,
    pub(super) workflow_nodes: Vec<crate::core::comfyui_workflow::ComfyWorkflowNode>,
    pub(super) workflow_error: Option<String>,
    pub(super) workflow_search: String,
    pub(super) selected_node_id: Option<String>,
    pub(super) comfy_schema: crate::core::comfyui_workflow::ComfyObjectInfoMap,
    pub(super) schema_base_url: Option<String>,
    pub(super) schema_status: Option<String>,
    pub(super) output_key: String,
    pub(super) output_tag: String,
    pub(super) output_node: Option<ProviderOutputNodeDraft>,
    pub(super) inputs: Vec<ProviderBuilderInput>,
    pub(super) tab: ProviderBuilderTab,
    pub(super) error: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ProviderOutputNodeDraft {
    pub(super) node_id: Option<String>,
    pub(super) class_type: String,
    pub(super) title: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ProviderNodeSelectorDraft {
    pub(super) node_id: Option<String>,
    pub(super) class_type: String,
    pub(super) input_key: String,
    pub(super) title: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ProviderBuilderInput {
    pub(super) name: String,
    pub(super) label: String,
    pub(super) input_type_key: String,
    pub(super) required: bool,
    pub(super) role: Option<InputRole>,
    pub(super) default_text: String,
    pub(super) enum_options: String,
    pub(super) tag: String,
    pub(super) multiline: bool,
    pub(super) ui_min: Option<f64>,
    pub(super) ui_max: Option<f64>,
    pub(super) ui_step: Option<f64>,
    pub(super) selector: ProviderNodeSelectorDraft,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ProviderInputAction {
    MoveUp(usize),
    MoveDown(usize),
    Delete(usize),
}

pub(super) struct ProviderBuilderSave {
    pub(super) entry: ProviderEntry,
    pub(super) manifest: ProviderManifest,
    pub(super) provider_path: PathBuf,
    pub(super) manifest_path: PathBuf,
}

impl Default for ProviderBuilderState {
    fn default() -> Self {
        Self::from_entry(None, crate::core::provider_store::default_provider_entry())
    }
}

impl ProviderBuilderState {
    pub(super) fn from_path(path: &Path) -> Self {
        let Some(json) = crate::core::provider_store::read_provider_file(path) else {
            let mut state = Self::default();
            state.source_path = Some(path.to_path_buf());
            state.error = Some(format!("Failed to read provider {}", path.display()));
            return state;
        };
        match serde_json::from_str::<ProviderEntry>(&json) {
            Ok(entry) => Self::from_entry(Some(path.to_path_buf()), entry),
            Err(err) => {
                let mut state = Self::default();
                state.source_path = Some(path.to_path_buf());
                state.error = Some(format!("Failed to parse provider JSON: {err}"));
                state
            }
        }
    }

    pub(super) fn from_entry(source_path: Option<PathBuf>, entry: ProviderEntry) -> Self {
        let (base_url, workflow_path, manifest_path) = match &entry.connection {
            ProviderConnection::ComfyUi {
                base_url,
                workflow_path,
                manifest_path,
            } => (
                base_url.clone(),
                workflow_path
                    .as_deref()
                    .map(|path| crate::core::paths::resolve_resource_path(Path::new(path))),
                manifest_path
                    .as_deref()
                    .map(|path| crate::core::paths::resolve_resource_path(Path::new(path))),
            ),
            ProviderConnection::OpenAiImage { base_url, .. }
            | ProviderConnection::XaiImage { base_url, .. }
            | ProviderConnection::XaiVideo { base_url, .. } => {
                (base_url.clone().unwrap_or_default(), None, None)
            }
            ProviderConnection::CustomHttp { base_url, .. } => (base_url.clone(), None, None),
        };

        let (workflow_nodes, workflow_error) = workflow_path
            .as_ref()
            .map(|path| match load_workflow_nodes_resolved(path) {
                Ok(nodes) => (nodes, None),
                Err(err) => (Vec::new(), Some(err)),
            })
            .unwrap_or_else(|| (Vec::new(), None));

        let mut state = Self {
            source_path,
            provider_id: entry.id,
            provider_name: entry.name.clone(),
            output_type: entry.output_type,
            workflow_kind: entry.workflow_kind,
            base_url,
            workflow_path,
            manifest_path: manifest_path.clone(),
            workflow_nodes,
            workflow_error,
            workflow_search: String::new(),
            selected_node_id: None,
            comfy_schema: crate::core::comfyui_workflow::ComfyObjectInfoMap::new(),
            schema_base_url: None,
            schema_status: None,
            output_key: default_output_key(entry.output_type).to_string(),
            output_tag: String::new(),
            output_node: None,
            inputs: entry
                .inputs
                .iter()
                .map(ProviderBuilderInput::from_provider_input)
                .collect(),
            tab: ProviderBuilderTab::Output,
            error: None,
        };

        if let Some(path) = manifest_path {
            match load_provider_manifest_resolved(&path) {
                Ok(manifest) => state.apply_manifest(manifest),
                Err(err) => state.error = Some(err),
            }
        }
        if state.workflow_nodes.is_empty() {
            if let Some(path) = state.workflow_path.as_ref() {
                match load_workflow_nodes_resolved(path) {
                    Ok(nodes) => {
                        state.workflow_nodes = nodes;
                        state.workflow_error = None;
                    }
                    Err(err) => state.workflow_error = Some(err),
                }
            }
        }
        state
    }

    pub(super) fn filtered_nodes(&self) -> Vec<crate::core::comfyui_workflow::ComfyWorkflowNode> {
        let query = self.workflow_search.trim().to_lowercase();
        if query.is_empty() {
            return self.workflow_nodes.clone();
        }
        self.workflow_nodes
            .iter()
            .filter(|node| {
                node.id.to_lowercase().contains(&query)
                    || node.class_type.to_lowercase().contains(&query)
                    || node
                        .title
                        .as_ref()
                        .is_some_and(|title| title.to_lowercase().contains(&query))
                    || node
                        .inputs
                        .iter()
                        .any(|input| input.to_lowercase().contains(&query))
            })
            .cloned()
            .collect()
    }

    pub(super) fn selected_node(&self) -> Option<crate::core::comfyui_workflow::ComfyWorkflowNode> {
        let selected_id = self.selected_node_id.as_ref()?;
        self.workflow_nodes
            .iter()
            .find(|node| &node.id == selected_id)
            .cloned()
    }

    pub(super) fn node_is_output(&self, node_id: &str) -> bool {
        self.output_node
            .as_ref()
            .and_then(|node| node.node_id.as_deref())
            .is_some_and(|id| id == node_id)
    }

    pub(super) fn exposed_input_count_for_node(&self, node_id: &str) -> usize {
        self.inputs
            .iter()
            .filter(|input| {
                input
                    .selector
                    .node_id
                    .as_deref()
                    .is_some_and(|id| id == node_id)
            })
            .count()
    }

    pub(super) fn input_exposed(&self, node_id: &str, input_key: &str) -> bool {
        self.inputs.iter().any(|input| {
            input
                .selector
                .node_id
                .as_deref()
                .is_some_and(|id| id == node_id)
                && input.selector.input_key == input_key
        })
    }

    pub(super) fn input_schema(
        &self,
        node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
        input_key: &str,
    ) -> Option<&crate::core::comfyui_workflow::ComfyInputSchema> {
        let schema_base_url = self.schema_base_url.as_deref()?;
        if normalize_comfy_base_url(schema_base_url) != normalize_comfy_base_url(&self.base_url) {
            return None;
        }
        self.comfy_schema
            .get(&node.class_type)
            .and_then(|inputs| inputs.get(input_key))
    }

    pub(super) fn enrich_existing_inputs_from_schema(&mut self) -> usize {
        let Some(schema_base_url) = self.schema_base_url.as_deref() else {
            return 0;
        };
        if normalize_comfy_base_url(schema_base_url) != normalize_comfy_base_url(&self.base_url) {
            return 0;
        }

        let mut updated_count = 0;
        for input in &mut self.inputs {
            let input_key = input.selector.input_key.clone();
            if input_key.trim().is_empty() {
                continue;
            }
            let node = input
                .selector
                .node_id
                .as_deref()
                .and_then(|node_id| self.workflow_nodes.iter().find(|node| node.id == node_id))
                .cloned()
                .unwrap_or_else(|| crate::core::comfyui_workflow::ComfyWorkflowNode {
                    id: input.selector.node_id.clone().unwrap_or_default(),
                    class_type: input.selector.class_type.clone(),
                    title: input.selector.title.clone(),
                    inputs: vec![input_key.clone()],
                    input_values: HashMap::new(),
                });

            let Some(schema) = self
                .comfy_schema
                .get(&node.class_type)
                .and_then(|inputs| inputs.get(&input_key))
            else {
                continue;
            };

            if input.apply_schema(&node, schema) {
                updated_count += 1;
            }
        }
        updated_count
    }

    pub(super) fn output_configured(&self) -> bool {
        self.output_node
            .as_ref()
            .and_then(|node| node.node_id.as_deref())
            .is_some_and(|node_id| !node_id.trim().is_empty())
    }

    pub(super) fn ensure_valid_tab(&mut self) {
        if !self.output_configured() && self.tab == ProviderBuilderTab::Inputs {
            self.tab = ProviderBuilderTab::Output;
        }
    }

    pub(super) fn reset_workflow_bindings(&mut self) {
        self.output_node = None;
        self.output_key = default_output_key(self.output_type).to_string();
        self.output_tag.clear();
        self.inputs.clear();
        self.tab = ProviderBuilderTab::Output;
    }

    pub(super) fn apply_manifest(&mut self, manifest: ProviderManifest) {
        match manifest {
            ProviderManifest::ComfyUi {
                name,
                output_type,
                workflow,
                inputs,
                output,
                ..
            } => {
                if let Some(name) = name {
                    self.provider_name = name;
                }
                self.output_type = output_type;
                self.workflow_path = Some(crate::core::paths::resolve_resource_path(Path::new(
                    &workflow.workflow_path,
                )));
                self.output_key = if output.selector.input_key.trim().is_empty() {
                    default_output_key(output_type).to_string()
                } else {
                    output.selector.input_key
                };
                self.output_tag = output.selector.tag.unwrap_or_default();
                self.output_node = Some(ProviderOutputNodeDraft {
                    node_id: output.selector.node_id,
                    class_type: output.selector.class_type,
                    title: output.selector.title,
                });
                self.inputs = inputs
                    .into_iter()
                    .map(ProviderBuilderInput::from_manifest_input)
                    .collect();
            }
            ProviderManifest::CustomHttp {
                name,
                output_type,
                inputs,
                ..
            } => {
                if let Some(name) = name {
                    self.provider_name = name;
                }
                self.output_type = output_type;
                self.inputs = inputs
                    .into_iter()
                    .map(ProviderBuilderInput::from_custom_http_input)
                    .collect();
                self.error = Some(
                    "Loaded a Custom HTTP manifest. Saving from this builder writes ComfyUI settings."
                        .to_string(),
                );
            }
        }
    }

    pub(super) fn output_status_label(&self) -> String {
        match self.output_node.as_ref() {
            Some(node)
                if node
                    .node_id
                    .as_deref()
                    .is_some_and(|node_id| !node_id.trim().is_empty()) =>
            {
                format!(
                    "Output: {} ({})",
                    node.title.clone().unwrap_or_else(|| "Untitled".to_string()),
                    node.class_type
                )
            }
            Some(_) => "Output: Re-select node".to_string(),
            None => "Output: Not set".to_string(),
        }
    }

    pub(super) fn workflow_path_display(&self) -> String {
        self.workflow_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Choose a workflow JSON".to_string())
    }

    pub(super) fn manifest_path_display(&self) -> String {
        self.manifest_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| {
                self.workflow_path
                    .as_ref()
                    .map(|path| derive_manifest_path(path).display().to_string())
                    .unwrap_or_else(|| "Derived from workflow on save".to_string())
            })
    }

    pub(super) fn role_validation_error(&self) -> Option<String> {
        let mut role_inputs: HashMap<InputRole, Vec<String>> = HashMap::new();
        let mut invalid_type_inputs = Vec::new();

        for input in &self.inputs {
            let Some(role) = input.role else {
                continue;
            };
            role_inputs.entry(role).or_default().push(input.name.clone());
            if !is_numeric_type_value(&input.input_type_key) {
                invalid_type_inputs.push(format!(
                    "{} ({}) set to {}",
                    input.name,
                    input.label,
                    provider_input_role_label(Some(role))
                ));
            }
        }

        let mut missing_roles = Vec::new();
        let required_roles = if self.output_type == ProviderOutputType::Audio {
            vec![InputRole::Seed]
        } else {
            vec![InputRole::Width, InputRole::Height, InputRole::Seed]
        };
        for role in required_roles {
            let names = role_inputs.get(&role).map_or(&[][..], |inputs| inputs.as_slice());
            if names.is_empty() {
                missing_roles.push(provider_input_role_label(Some(role)).to_string());
                continue;
            }
            if names.len() > 1 {
                return Some(format!(
                    "Role {} is assigned to multiple inputs ({}). Only one input can be marked as {}.",
                    provider_input_role_label(Some(role)),
                    names.join(", "),
                    provider_input_role_label(Some(role)).to_ascii_lowercase()
                ));
            }
        }
        if !invalid_type_inputs.is_empty() {
            return Some(format!(
                "Invalid role assignment: {}. Roles must be set on a number/integer input.",
                invalid_type_inputs.join(", ")
            ));
        }
        if !missing_roles.is_empty() {
            Some(format!("Missing required roles: {}.", missing_roles.join(", ")))
        } else {
            None
        }
    }

    pub(super) fn build_save_payload(&self) -> Result<ProviderBuilderSave, String> {
        if let Some(error) = self.role_validation_error() {
            return Err(error);
        }
        let workflow_path = self
            .workflow_path
            .clone()
            .ok_or_else(|| "Select a workflow first.".to_string())?;
        let provider_name = self.provider_name.trim();
        if provider_name.is_empty() {
            return Err("Provider name is required.".to_string());
        }
        let base_url = self.base_url.trim();
        if base_url.is_empty() {
            return Err("Base URL is required.".to_string());
        }
        let output_node = self
            .output_node
            .clone()
            .ok_or_else(|| "Select an output node.".to_string())?;
        let output_key = inferred_output_key_for_node(&output_node, self.output_type);

        let mut manifest_inputs = Vec::new();
        let mut provider_inputs = Vec::new();
        for input in &self.inputs {
            let input_type = parse_provider_input_type(input)?;
            let default = parse_provider_default_value(&input_type, &input.default_text)?;
            let tag = input.tag.trim();
            let selector = NodeSelector {
                node_id: input.selector.node_id.clone(),
                tag: if tag.is_empty() {
                    None
                } else {
                    Some(tag.to_string())
                },
                class_type: input.selector.class_type.clone(),
                input_key: input.selector.input_key.clone(),
                title: input.selector.title.clone(),
            };
            if selector
                .node_id
                .as_ref()
                .is_none_or(|node_id| node_id.trim().is_empty())
                || selector.input_key.trim().is_empty()
            {
                return Err(format!(
                    "Input '{}' needs a node_id workflow binding. Select a node and expose the input again.",
                    input.name
                ));
            }

            let input_ui = build_provider_input_ui(input);
            manifest_inputs.push(ManifestInput {
                name: input.name.clone(),
                label: input.label.clone(),
                input_type: input_type.clone(),
                required: input.required,
                default: default.clone(),
                role: input.role,
                ui: input_ui.clone(),
                bind: InputBinding {
                    selector,
                    transform: None,
                },
            });
            provider_inputs.push(ProviderInputField {
                name: input.name.clone(),
                label: input.label.clone(),
                input_type,
                required: input.required,
                default,
                role: input.role,
                ui: input_ui,
            });
        }

        let output_tag = self.output_tag.trim();
        let output_selector = NodeSelector {
            node_id: output_node.node_id,
            tag: if output_tag.is_empty() {
                None
            } else {
                Some(output_tag.to_string())
            },
            class_type: output_node.class_type,
            input_key: output_key,
            title: output_node.title,
        };
        if output_selector
            .node_id
            .as_ref()
            .is_none_or(|node_id| node_id.trim().is_empty())
        {
            return Err(
                "Output node needs a node_id binding. Select the output node again.".to_string(),
            );
        }

        let manifest_path = self
            .manifest_path
            .clone()
            .unwrap_or_else(|| derive_manifest_path(&workflow_path));
        let provider_path = self.source_path.clone().unwrap_or_else(|| {
            crate::core::provider_store::provider_path_for_entry(&ProviderEntry {
                id: self.provider_id,
                name: provider_name.to_string(),
                output_type: self.output_type,
                workflow_kind: self.workflow_kind,
                inputs: Vec::new(),
                connection: ProviderConnection::ComfyUi {
                    base_url: base_url.to_string(),
                    workflow_path: Some(crate::core::paths::storage_resource_path(&workflow_path)),
                    manifest_path: Some(crate::core::paths::storage_resource_path(&manifest_path)),
                },
            })
        });

        let workflow_path_string = crate::core::paths::storage_resource_path(&workflow_path);
        let manifest_path_string = crate::core::paths::storage_resource_path(&manifest_path);
        let manifest = ProviderManifest::ComfyUi {
            schema_version: 1,
            name: Some(provider_name.to_string()),
            output_type: self.output_type,
            workflow: ComfyWorkflowRef {
                workflow_path: workflow_path_string.clone(),
                workflow_hash: None,
            },
            inputs: manifest_inputs,
            output: ComfyOutputSelector {
                selector: output_selector,
                index: None,
            },
        };
        let entry = ProviderEntry {
            id: self.provider_id,
            name: provider_name.to_string(),
            output_type: self.output_type,
            workflow_kind: self.workflow_kind,
            inputs: provider_inputs,
            connection: ProviderConnection::ComfyUi {
                base_url: base_url.to_string(),
                workflow_path: Some(workflow_path_string),
                manifest_path: Some(manifest_path_string),
            },
        };

        Ok(ProviderBuilderSave {
            entry,
            manifest,
            provider_path,
            manifest_path,
        })
    }
}

impl ProviderBuilderInput {
    pub(super) fn from_node(
        node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
        input_key: &str,
        name: String,
        label: String,
        schema: Option<&crate::core::comfyui_workflow::ComfyInputSchema>,
    ) -> Self {
        let (heuristic_key, heuristic_multiline) =
            infer_provider_input_from_workflow_node(node, input_key);
        let input_type_key =
            schema_provider_input_type_key(node, input_key, schema, &heuristic_key);
        let default_value = node
            .input_values
            .get(input_key)
            .filter(|value| provider_builder_default_value_is_scalar(value))
            .or_else(|| schema.and_then(|schema| schema.default.as_ref()));
        let enum_options = if input_type_key == "enum" {
            schema_or_workflow_enum_options(node, input_key, schema).join("\n")
        } else {
            String::new()
        };
        let default_text =
            provider_builder_default_text_for_type(&input_type_key, default_value, &enum_options);
        let multiline =
            schema.map(|schema| schema.multiline).unwrap_or(false) || heuristic_multiline;
        Self {
            name,
            label,
            input_type_key,
            required: schema.map(|schema| schema.required).unwrap_or(false),
            role: None,
            default_text,
            enum_options,
            tag: String::new(),
            multiline,
            ui_min: schema.and_then(|schema| schema.min),
            ui_max: schema.and_then(|schema| schema.max),
            ui_step: schema.and_then(|schema| schema.step),
            selector: ProviderNodeSelectorDraft {
                node_id: Some(node.id.clone()),
                class_type: node.class_type.clone(),
                input_key: input_key.to_string(),
                title: node.title.clone(),
            },
        }
    }

    pub(super) fn from_provider_input(input: &ProviderInputField) -> Self {
        let (input_type_key, enum_options) = provider_input_type_to_key(&input.input_type);
        let ui_meta = input.ui.as_ref();
        Self {
            name: input.name.clone(),
            label: input.label.clone(),
            input_type_key,
            required: input.required,
            default_text: default_value_to_text(input.default.as_ref()),
            enum_options,
            role: input.role,
            tag: String::new(),
            multiline: ui_meta.is_some_and(|ui| ui.multiline),
            ui_min: ui_meta.and_then(|ui| ui.min),
            ui_max: ui_meta.and_then(|ui| ui.max),
            ui_step: ui_meta.and_then(|ui| ui.step),
            selector: ProviderNodeSelectorDraft {
                node_id: None,
                class_type: String::new(),
                input_key: input.name.clone(),
                title: None,
            },
        }
    }

    pub(super) fn from_manifest_input(input: ManifestInput) -> Self {
        let (input_type_key, enum_options) = provider_input_type_to_key(&input.input_type);
        let ui_min = input.ui.as_ref().and_then(|ui| ui.min);
        let ui_max = input.ui.as_ref().and_then(|ui| ui.max);
        let ui_step = input.ui.as_ref().and_then(|ui| ui.step);
        let multiline = input.ui.as_ref().is_some_and(|ui| ui.multiline);
        Self {
            name: input.name,
            label: input.label,
            input_type_key,
            required: input.required,
            default_text: default_value_to_text(input.default.as_ref()),
            enum_options,
            role: input.role,
            tag: input.bind.selector.tag.unwrap_or_default(),
            multiline,
            ui_min,
            ui_max,
            ui_step,
            selector: ProviderNodeSelectorDraft {
                node_id: input.bind.selector.node_id,
                class_type: input.bind.selector.class_type,
                input_key: input.bind.selector.input_key,
                title: input.bind.selector.title,
            },
        }
    }

    pub(super) fn from_custom_http_input(input: crate::state::CustomHttpInput) -> Self {
        let (input_type_key, enum_options) = provider_input_type_to_key(&input.input_type);
        let ui_min = input.ui.as_ref().and_then(|ui| ui.min);
        let ui_max = input.ui.as_ref().and_then(|ui| ui.max);
        let ui_step = input.ui.as_ref().and_then(|ui| ui.step);
        let multiline = input.ui.as_ref().is_some_and(|ui| ui.multiline);
        Self {
            name: input.name,
            label: input.label,
            input_type_key,
            required: input.required,
            default_text: default_value_to_text(input.default.as_ref()),
            enum_options,
            role: input.role,
            tag: String::new(),
            multiline,
            ui_min,
            ui_max,
            ui_step,
            selector: ProviderNodeSelectorDraft {
                node_id: None,
                class_type: String::new(),
                input_key: String::new(),
                title: None,
            },
        }
    }

    pub(super) fn apply_schema(
        &mut self,
        node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
        schema: &crate::core::comfyui_workflow::ComfyInputSchema,
    ) -> bool {
        let before = (
            self.input_type_key.clone(),
            self.required,
            self.default_text.clone(),
            self.enum_options.clone(),
            self.role,
            self.multiline,
            self.ui_min,
            self.ui_max,
            self.ui_step,
        );
        let (heuristic_key, heuristic_multiline) =
            infer_provider_input_from_workflow_node(node, &self.selector.input_key);
        let next_type = schema_provider_input_type_key(
            node,
            &self.selector.input_key,
            Some(schema),
            &heuristic_key,
        );
        let default_value = node
            .input_values
            .get(&self.selector.input_key)
            .filter(|value| provider_builder_default_value_is_scalar(value))
            .or(schema.default.as_ref());

        self.input_type_key = next_type;
        self.required = schema.required;
        self.multiline = schema.multiline || heuristic_multiline;
        self.ui_min = schema.min;
        self.ui_max = schema.max;
        self.ui_step = schema.step;

        if self.input_type_key == "enum" {
            let schema_options =
                schema_or_workflow_enum_options(node, &self.selector.input_key, Some(schema))
                    .join("\n");
            if !schema_options.trim().is_empty() {
                self.enum_options = schema_options;
            }
            let options = provider_builder_enum_options(self);
            let current_default = self.default_text.trim();
            if current_default.is_empty() || !options.iter().any(|option| option == current_default)
            {
                self.default_text = provider_builder_default_text_for_type(
                    &self.input_type_key,
                    default_value,
                    &self.enum_options,
                );
            }
        } else {
            self.enum_options.clear();
            if matches!(self.input_type_key.as_str(), "image" | "video" | "audio") {
                self.default_text.clear();
                self.multiline = false;
            } else if self.default_text.trim().is_empty() {
                self.default_text = provider_builder_default_text_for_type(
                    &self.input_type_key,
                    default_value,
                    &self.enum_options,
                );
            }
        }
        before
            != (
                self.input_type_key.clone(),
                self.required,
                self.default_text.clone(),
                self.enum_options.clone(),
                self.role,
                self.multiline,
                self.ui_min,
                self.ui_max,
                self.ui_step,
            )
    }
}

pub(super) struct ProviderFileSummary {
    pub(super) name: String,
    pub(super) subtitle: String,
    pub(super) output_type: Option<ProviderOutputType>,
}

pub(super) fn provider_row(
    ui: &mut Ui,
    _path: &Path,
    summary: &ProviderFileSummary,
    selected: bool,
) -> egui::Response {
    let accent = summary
        .output_type
        .map(provider_output_color)
        .unwrap_or(kit::AUDIO);
    kit::draw_accent_row(ui, 52.0, selected, accent, |ui, rect| {
        paint_text_button_row(ui, rect, &summary.name, &summary.subtitle);
    })
}

pub(super) fn provider_template_dropdown_label(
    kind: ProviderTemplateKind,
    unavailable: bool,
) -> String {
    if unavailable {
        format!("{} (already added)", provider_template_label(kind))
    } else {
        provider_template_label(kind).to_string()
    }
}

pub(super) fn provider_template_label(kind: ProviderTemplateKind) -> &'static str {
    match kind {
        ProviderTemplateKind::ComfyUi => "ComfyUI Workflow",
        ProviderTemplateKind::OpenAiImage => "OpenAI Image",
        ProviderTemplateKind::XaiImage => "xAI Image",
        ProviderTemplateKind::XaiVideo => "xAI Grok Video",
    }
}

pub(super) fn provider_file_credential(path: &Path) -> Option<(&'static str, &'static str)> {
    let text = std::fs::read_to_string(path).ok()?;
    let entry = serde_json::from_str::<ProviderEntry>(&text).ok()?;
    match entry.connection {
        ProviderConnection::OpenAiImage { .. } => {
            Some((crate::core::credentials::OPENAI_CREDENTIAL_ID, "OpenAI"))
        }
        ProviderConnection::XaiImage { .. } => {
            Some((crate::core::credentials::XAI_CREDENTIAL_ID, "xAI"))
        }
        ProviderConnection::XaiVideo { .. } => {
            Some((crate::core::credentials::XAI_CREDENTIAL_ID, "xAI"))
        }
        ProviderConnection::ComfyUi { .. } | ProviderConnection::CustomHttp { .. } => None,
    }
}

pub(super) fn paint_text_button_row(ui: &mut Ui, rect: Rect, title: &str, subtitle: &str) {
    let text_width = rect.width().max(24.0);
    paint_truncated_row_text_top(
        ui,
        Pos2::new(rect.left(), rect.top() + 2.0),
        kit::value(title),
        12.0,
        text_width,
        kit::TEXT,
    );
    paint_truncated_row_text_bottom(
        ui,
        Pos2::new(rect.left(), rect.bottom() - 2.0),
        kit::caption(subtitle),
        11.0,
        text_width,
        kit::TEXT_MUTED,
    );
}

pub(super) fn provider_file_summary(path: &Path) -> ProviderFileSummary {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("provider.json")
        .to_string();
    let Ok(text) = std::fs::read_to_string(path) else {
        return ProviderFileSummary {
            name: file_name,
            subtitle: "Unreadable provider file".to_string(),
            output_type: None,
        };
    };
    let Ok(entry) = serde_json::from_str::<ProviderEntry>(&text) else {
        return ProviderFileSummary {
            name: file_name,
            subtitle: "Invalid provider JSON".to_string(),
            output_type: None,
        };
    };
    let workflow_kind = entry.resolved_workflow_kind();
    ProviderFileSummary {
        name: entry.name,
        subtitle: format!(
            "{} {}  {}",
            workflow_kind.short_label(),
            provider_output_type_label(entry.output_type),
            path_label(path)
        ),
        output_type: Some(entry.output_type),
    }
}

pub(super) fn provider_file_supports_comfy_builder(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(entry) = serde_json::from_str::<ProviderEntry>(&text) else {
        return false;
    };
    matches!(entry.connection, ProviderConnection::ComfyUi { .. })
}

pub(super) fn provider_output_color(output_type: ProviderOutputType) -> Color32 {
    match output_type {
        ProviderOutputType::Image => kit::IMAGE,
        ProviderOutputType::Video => kit::VIDEO,
        ProviderOutputType::Audio => kit::AUDIO,
    }
}

pub(super) fn provider_output_type_label(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "Image",
        ProviderOutputType::Video => "Video",
        ProviderOutputType::Audio => "Audio",
    }
}

pub(super) fn clip_image_mode_label(mode: ClipImageMode) -> &'static str {
    match mode {
        ClipImageMode::Still => "Still Image",
        ClipImageMode::Keyframe => "Keyframe Reference",
    }
}

pub(super) fn provider_output_type_field(ui: &mut Ui, label: &str, value: &mut ProviderOutputType) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        let width = ui.available_width();
        kit::configure_field_widget_style(ui, width);
        let combo_id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);
        egui::ComboBox::from_id_salt(combo_id)
            .width(width)
            .selected_text(provider_output_type_label(*value))
            .show_ui(ui, |ui| {
                automation_selectable_value(ui, value, ProviderOutputType::Image, "Image");
                automation_selectable_value(ui, value, ProviderOutputType::Video, "Video");
                automation_selectable_value(ui, value, ProviderOutputType::Audio, "Audio");
            });
    });
}

pub(super) fn provider_workflow_kind_field(
    ui: &mut Ui,
    label: &str,
    value: &mut ProviderWorkflowKind,
) {
    kit::labeled_combo_field(ui, label, "provider_workflow_kind", value.label(), |ui| {
        for kind in ProviderWorkflowKind::ALL {
            automation_selectable_value(ui, value, kind, kind.label());
        }
    });
}

pub(super) fn workflow_node_row(
    ui: &mut Ui,
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    selected: bool,
    output_selected: bool,
    exposed_input_count: usize,
    output_type: ProviderOutputType,
) -> egui::Response {
    let status_accent = if output_selected {
        Some(provider_output_color(output_type))
    } else if exposed_input_count > 0 {
        Some(kit::BORDER_FOCUS)
    } else {
        None
    };
    kit::draw_accent_row_with_status(ui, 54.0, selected, kit::IMAGE, status_accent, |ui, rect| {
        let title = node.title.as_deref().unwrap_or("Untitled");
        let status = match (output_selected, exposed_input_count) {
            (true, 0) => "  Output".to_string(),
            (true, 1) => "  Output + 1 input".to_string(),
            (true, count) => format!("  Output + {count} inputs"),
            (false, 1) => "  1 input exposed".to_string(),
            (false, count) if count > 1 => format!("  {count} inputs exposed"),
            _ => String::new(),
        };
        let subtitle = format!("{}  Node {}{}", node.class_type, node.id, status);
        paint_text_button_row(ui, rect, title, &subtitle);
    })
}

pub(super) fn provider_builder_input_editor(
    ui: &mut Ui,
    index: usize,
    len: usize,
    input: &mut ProviderBuilderInput,
    action: &mut Option<ProviderInputAction>,
) {
    let card_w = ui.available_width().max(0.0);
    ui.scope(|ui| {
        ui.set_width(card_w);
        ui.set_min_width(card_w);
        ui.set_max_width(card_w);
        kit::sunken_frame().show(ui, |ui| {
            let content_w = (card_w - 16.0).max(0.0);
            ui.set_width(content_w);
            ui.set_min_width(content_w);
            ui.set_max_width(content_w);
            provider_builder_input_editor_contents(ui, index, len, input, action);
        });
    });
}

pub(super) fn provider_builder_input_editor_contents(
    ui: &mut Ui,
    index: usize,
    len: usize,
    input: &mut ProviderBuilderInput,
    action: &mut Option<ProviderInputAction>,
) {
    kit::field_grid_row(ui, &[1.0, 1.0], |ui, column| match column {
        0 => {
            kit::labeled_text_field(ui, "Name", &mut input.name);
        }
        1 => {
            kit::labeled_text_field(ui, "Label", &mut input.label);
        }
        _ => {}
    });
    ui.add_space(kit::FORM_ROW_GAP);
    kit::field_grid_row(ui, &[0.44, 1.0], |ui, column| match column {
        0 => {
            provider_input_type_field(ui, "Type", &mut input.input_type_key);
        }
        1 => {
            provider_builder_default_field(ui, input);
        }
        _ => {}
    });
    ui.add_space(kit::FORM_ROW_GAP);
    provider_input_role_field(ui, "Role", &input.name, &mut input.role);
    ui.add_space(kit::FORM_ROW_GAP);
    if input.input_type_key == "enum" {
        kit::field_label(ui, "Enum Options");
        ui.add_space(kit::FIELD_LABEL_GAP);
        kit::multiline_text_field(
            ui,
            &mut input.enum_options,
            ui.available_width(),
            kit::MultilineTextFieldOptions::rows(3),
        );
        ui.add_space(kit::FORM_ROW_GAP);
    }
    kit::field_grid_row(ui, &[1.0, 1.0], |ui, column| match column {
        0 => {
            kit::labeled_text_field(ui, "Tag", &mut input.tag);
        }
        1 => {
            ui.add_space(kit::FIELD_LABEL_H + kit::FIELD_LABEL_GAP);
            ui.horizontal(|ui| {
                automation_checkbox(ui, &mut input.required, "Required");
                if input.input_type_key == "text" {
                    automation_checkbox(ui, &mut input.multiline, "Multiline");
                } else {
                    input.multiline = false;
                }
            });
        }
        _ => {}
    });
    ui.add_space(kit::FORM_ROW_GAP);
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let buttons_w = 42.0 + 52.0 + 66.0 + gap * 3.0;
        ui.add_sized(
            [(ui.available_width() - buttons_w).max(0.0), 18.0],
            egui::Label::new(kit::caption(format!(
                "-> node {} / {}.{}",
                input.selector.node_id.as_deref().unwrap_or("-"),
                empty_dash(&input.selector.class_type),
                empty_dash(&input.selector.input_key)
            )))
            .truncate(),
        );
        if kit::field_button(ui, "Up", 42.0).clicked() && index > 0 {
            *action = Some(ProviderInputAction::MoveUp(index));
        }
        if kit::field_button(ui, "Down", 52.0).clicked() && index + 1 < len {
            *action = Some(ProviderInputAction::MoveDown(index));
        }
        if kit::danger_button(ui, "Delete", 66.0).clicked() {
            *action = Some(ProviderInputAction::Delete(index));
        }
    });
}

pub(super) fn provider_input_role_label(value: Option<InputRole>) -> &'static str {
    match value {
        Some(InputRole::Width) => "Width",
        Some(InputRole::Height) => "Height",
        Some(InputRole::Seed) => "Seed",
        None => "None",
    }
}

pub(super) fn provider_input_role_field(
    ui: &mut Ui,
    label: &str,
    input_name: &str,
    role: &mut Option<InputRole>,
) {
    kit::labeled_combo_field(
        ui,
        label,
        ("provider_input_role", input_name),
        provider_input_role_label(*role),
        |ui| {
            automation_selectable_value(ui, role, None, "None");
            automation_selectable_value(ui, role, Some(InputRole::Width), "Width");
            automation_selectable_value(ui, role, Some(InputRole::Height), "Height");
            automation_selectable_value(ui, role, Some(InputRole::Seed), "Seed");
        },
    );
}

pub(super) fn provider_builder_default_field(ui: &mut Ui, input: &mut ProviderBuilderInput) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, "Default");
        match input.input_type_key.as_str() {
            "boolean" => {
                let mut value = parse_bool_default_text(&input.default_text).unwrap_or(false);
                let response = automation_checkbox(ui, &mut value, "True");
                if response.changed() {
                    input.default_text = value.to_string();
                }
            }
            "integer" => {
                let mut value = input.default_text.trim().parse::<i64>().unwrap_or(0);
                let width = ui.available_width();
                let rect = inspector_numeric_rect(ui, width);
                if provider_builder_integer_default_in_rect(ui, rect, &mut value) {
                    input.default_text = value.to_string();
                }
            }
            "number" => {
                let mut value = input.default_text.trim().parse::<f64>().unwrap_or(0.0);
                let width = ui.available_width();
                let rect = inspector_numeric_rect(ui, width);
                if provider_builder_number_default_in_rect(ui, rect, &mut value) {
                    input.default_text = value.to_string();
                }
            }
            "enum" => {
                let options = provider_builder_enum_options(input);
                if options.is_empty() {
                    kit::singleline_text_field(ui, &mut input.default_text, ui.available_width());
                } else {
                    if input.default_text.trim().is_empty() {
                        input.default_text = options[0].clone();
                    }
                    let selected = input.default_text.clone();
                    kit::combo_field(
                        ui,
                        ("provider_default_enum", &input.name),
                        selected,
                        ui.available_width(),
                        |ui| {
                            for option in options {
                                automation_selectable_value(
                                    ui,
                                    &mut input.default_text,
                                    option.clone(),
                                    &option,
                                );
                            }
                        },
                    );
                }
            }
            "image" | "video" | "audio" => {
                input.default_text.clear();
                kit::readonly_value_box(
                    ui,
                    "Runtime asset binding",
                    Vec2::new(ui.available_width(), kit::FIELD_H),
                );
            }
            _ => {
                kit::singleline_text_field(ui, &mut input.default_text, ui.available_width());
            }
        }
    });
}

pub(super) fn provider_builder_integer_default_in_rect(
    ui: &mut Ui,
    rect: Rect,
    value: &mut i64,
) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value).speed(1.0),
        )
    })
}

pub(super) fn provider_builder_number_default_in_rect(
    ui: &mut Ui,
    rect: Rect,
    value: &mut f64,
) -> bool {
    inspector_numeric_field(ui, rect, |ui, width| {
        ui.add_sized(
            [width, INSPECTOR_NUMERIC_H],
            egui::DragValue::new(value).speed(0.1),
        )
    })
}

pub(super) fn provider_input_type_field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        let width = ui.available_width();
        kit::configure_field_widget_style(ui, width);
        let combo_id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);
        egui::ComboBox::from_id_salt(combo_id)
            .width(width)
            .selected_text(provider_input_type_label(value))
            .show_ui(ui, |ui| {
                for (key, label) in [
                    ("text", "Text"),
                    ("number", "Number"),
                    ("integer", "Integer"),
                    ("boolean", "Boolean"),
                    ("enum", "Enum"),
                    ("image", "Image"),
                    ("video", "Video"),
                    ("audio", "Audio"),
                ] {
                    automation_selectable_value(ui, value, key.to_string(), label);
                }
            });
    });
}

pub(super) fn provider_input_type_label(value: &str) -> &'static str {
    match value {
        "number" => "Number",
        "integer" => "Integer",
        "boolean" => "Boolean",
        "enum" => "Enum",
        "image" => "Image",
        "video" => "Video",
        "audio" => "Audio",
        _ => "Text",
    }
}

pub(super) fn provider_input_type_to_key(input_type: &ProviderInputType) -> (String, String) {
    match input_type {
        ProviderInputType::Text => ("text".to_string(), String::new()),
        ProviderInputType::Number => ("number".to_string(), String::new()),
        ProviderInputType::Integer => ("integer".to_string(), String::new()),
        ProviderInputType::Boolean => ("boolean".to_string(), String::new()),
        ProviderInputType::Enum { options } => ("enum".to_string(), options.join("\n")),
        ProviderInputType::Image => ("image".to_string(), String::new()),
        ProviderInputType::Video => ("video".to_string(), String::new()),
        ProviderInputType::Audio => ("audio".to_string(), String::new()),
    }
}

pub(super) fn parse_provider_input_type(
    input: &ProviderBuilderInput,
) -> Result<ProviderInputType, String> {
    match input.input_type_key.as_str() {
        "text" => Ok(ProviderInputType::Text),
        "number" => Ok(ProviderInputType::Number),
        "integer" => Ok(ProviderInputType::Integer),
        "boolean" => Ok(ProviderInputType::Boolean),
        "image" => Ok(ProviderInputType::Image),
        "video" => Ok(ProviderInputType::Video),
        "audio" => Ok(ProviderInputType::Audio),
        "enum" => {
            let options = provider_builder_enum_options(input);
            if options.is_empty() {
                Err(format!(
                    "Enum input '{}' needs at least one option.",
                    input.name
                ))
            } else {
                Ok(ProviderInputType::Enum { options })
            }
        }
        other => Err(format!("Unknown input type: {other}")),
    }
}

fn is_numeric_type_value(input_type: &str) -> bool {
    matches!(input_type, "number" | "integer")
}

pub(super) fn parse_provider_default_value(
    input_type: &ProviderInputType,
    text: &str,
) -> Result<Option<serde_json::Value>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let value = match input_type {
        ProviderInputType::Text => serde_json::Value::String(trimmed.to_string()),
        ProviderInputType::Number => {
            let parsed = trimmed
                .parse::<f64>()
                .map_err(|_| format!("Invalid number default '{trimmed}'."))?;
            serde_json::Value::Number(
                serde_json::Number::from_f64(parsed)
                    .ok_or_else(|| format!("Invalid number default '{trimmed}'."))?,
            )
        }
        ProviderInputType::Integer => {
            let parsed = trimmed
                .parse::<i64>()
                .map_err(|_| format!("Invalid integer default '{trimmed}'."))?;
            serde_json::Value::Number(parsed.into())
        }
        ProviderInputType::Boolean => {
            let parsed = parse_bool_default_text(trimmed)
                .map_err(|_| format!("Invalid boolean default '{trimmed}'."))?;
            serde_json::Value::Bool(parsed)
        }
        ProviderInputType::Enum { .. } => serde_json::Value::String(trimmed.to_string()),
        ProviderInputType::Image | ProviderInputType::Video | ProviderInputType::Audio => {
            return Ok(None)
        }
    };
    Ok(Some(value))
}

pub(super) fn parse_bool_default_text(text: &str) -> Result<bool, ()> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" | "1" => Ok(true),
        "false" | "f" | "no" | "n" | "off" | "0" => Ok(false),
        _ => Err(()),
    }
}

pub(super) fn provider_builder_enum_options(input: &ProviderBuilderInput) -> Vec<String> {
    split_provider_builder_enum_options(&input.enum_options)
}

pub(super) fn split_provider_builder_enum_options(text: &str) -> Vec<String> {
    let has_newlines = text.contains('\n') || text.contains('\r');
    let values: Vec<&str> = if has_newlines {
        text.lines().collect()
    } else {
        text.split(',').collect()
    };
    values
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn build_provider_input_ui(input: &ProviderBuilderInput) -> Option<InputUi> {
    if (input.input_type_key == "text" && input.multiline)
        || input.ui_min.is_some()
        || input.ui_max.is_some()
        || input.ui_step.is_some()
    {
        Some(InputUi {
            min: input.ui_min,
            max: input.ui_max,
            step: input.ui_step,
            placeholder: None,
            multiline: input.input_type_key == "text" && input.multiline,
            group: None,
            advanced: false,
            unit: None,
        })
    } else {
        None
    }
}

pub(super) fn normalize_comfy_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_ascii_lowercase()
}

pub(super) fn schema_provider_input_type_key(
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    input_key: &str,
    schema: Option<&crate::core::comfyui_workflow::ComfyInputSchema>,
    heuristic_key: &str,
) -> String {
    if matches!(heuristic_key, "image" | "video" | "audio") {
        return heuristic_key.to_string();
    }

    let Some(schema) = schema else {
        return heuristic_key.to_string();
    };

    if !schema.enum_options.is_empty() {
        return "enum".to_string();
    }

    let Some(type_name) = schema.type_name.as_deref() else {
        return heuristic_key.to_string();
    };
    match type_name.trim().to_ascii_uppercase().as_str() {
        "INT" | "INTEGER" => "integer".to_string(),
        "FLOAT" | "DOUBLE" | "NUMBER" => "number".to_string(),
        "BOOLEAN" | "BOOL" => "boolean".to_string(),
        "STRING" | "TEXT" => "text".to_string(),
        "IMAGE" | "MASK" => "image".to_string(),
        "VIDEO" => "video".to_string(),
        "AUDIO" => "audio".to_string(),
        "ENUM" | "COMBO" => "enum".to_string(),
        _ => infer_provider_input_from_workflow_node(node, input_key).0,
    }
}

pub(super) fn schema_or_workflow_enum_options(
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    input_key: &str,
    schema: Option<&crate::core::comfyui_workflow::ComfyInputSchema>,
) -> Vec<String> {
    if let Some(schema) = schema {
        if !schema.enum_options.is_empty() {
            return schema.enum_options.clone();
        }
        let schema_is_enum = schema.type_name.as_deref().is_some_and(|type_name| {
            matches!(
                type_name.trim().to_ascii_uppercase().as_str(),
                "ENUM" | "COMBO"
            )
        });
        if schema_is_enum {
            let workflow_options = workflow_node_enum_options(node, input_key);
            if !workflow_options.is_empty() {
                return workflow_options;
            }
        }
    }

    if node.class_type == "CustomCombo" || input_key.eq_ignore_ascii_case("choice") {
        return workflow_node_enum_options(node, input_key);
    }

    Vec::new()
}

pub(super) fn workflow_node_enum_options(
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    _input_key: &str,
) -> Vec<String> {
    let mut numbered = Vec::new();
    let mut unnumbered = Vec::new();
    for (key, value) in node.input_values.iter() {
        let key_lower = key.to_ascii_lowercase();
        if !key_lower.starts_with("option") {
            continue;
        }
        let Some(option) = workflow_option_value_to_string(value) else {
            continue;
        };
        if option.trim().is_empty() {
            continue;
        }
        let suffix = key_lower.trim_start_matches("option");
        if let Ok(index) = suffix.parse::<usize>() {
            numbered.push((index, option));
        } else {
            unnumbered.push((key_lower, option));
        }
    }

    numbered.sort_by_key(|(index, _)| *index);
    unnumbered.sort_by(|(left, _), (right, _)| left.cmp(right));
    let mut options = Vec::new();
    for (_, option) in numbered {
        if !options.contains(&option) {
            options.push(option);
        }
    }
    for (_, option) in unnumbered {
        if !options.contains(&option) {
            options.push(option);
        }
    }
    options
}

fn workflow_option_value_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

pub(super) fn provider_builder_default_value_is_scalar(value: &serde_json::Value) -> bool {
    matches!(
        value,
        serde_json::Value::String(_) | serde_json::Value::Number(_) | serde_json::Value::Bool(_)
    )
}

pub(super) fn provider_builder_default_text_for_type(
    input_type_key: &str,
    value: Option<&serde_json::Value>,
    enum_options: &str,
) -> String {
    if matches!(input_type_key, "image" | "video" | "audio") {
        return String::new();
    }
    let value_text = default_value_to_text(value);
    if !value_text.trim().is_empty() {
        return value_text;
    }
    if input_type_key == "enum" {
        return split_provider_builder_enum_options(enum_options)
            .into_iter()
            .next()
            .unwrap_or_default();
    }
    String::new()
}

pub(super) fn default_value_to_text(value: Option<&serde_json::Value>) -> String {
    value
        .map(|value| match value {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::Bool(flag) => flag.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

pub(super) fn derive_manifest_path(workflow_path: &Path) -> PathBuf {
    let mut path = workflow_path.to_path_buf();
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("workflow");
    path.set_file_name(format!("{stem}_manifest.json"));
    path
}

pub(super) fn provider_name_from_workflow_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn load_workflow_nodes_resolved(
    path: &Path,
) -> Result<Vec<crate::core::comfyui_workflow::ComfyWorkflowNode>, String> {
    let resolved = crate::core::paths::resolve_resource_path(path);
    crate::core::comfyui_workflow::load_workflow_nodes(&resolved)
}

pub(super) fn load_provider_manifest_resolved(path: &Path) -> Result<ProviderManifest, String> {
    let resolved = crate::core::paths::resolve_resource_path(path);
    let text = std::fs::read_to_string(&resolved)
        .map_err(|err| format!("Failed to read manifest {}: {err}", path.display()))?;
    serde_json::from_str::<ProviderManifest>(&text)
        .map_err(|err| format!("Failed to parse manifest {}: {err}", path.display()))
}

pub(super) fn friendly_provider_label(name: &str) -> String {
    name.replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn provider_input_name_and_label(
    node_title: Option<&str>,
    input_key: &str,
    inputs: &[ProviderBuilderInput],
) -> (String, String) {
    let existing = inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect::<HashSet<_>>();
    let key_base = sanitize_provider_input_name(input_key).unwrap_or_else(|| "input".to_string());
    let use_title = is_generic_provider_input_key(input_key)
        || existing.contains(key_base.as_str())
        || input_key.trim().is_empty();
    let title_base = node_title
        .and_then(sanitize_provider_input_name)
        .filter(|value| !value.is_empty());
    let base = if use_title {
        title_base.unwrap_or(key_base)
    } else {
        key_base
    };
    if !existing.contains(base.as_str()) {
        let label_source = if use_title {
            node_title.unwrap_or(input_key)
        } else {
            input_key
        };
        return (base, friendly_provider_label(label_source));
    }
    for index in 2.. {
        let candidate = format!("{base}_{index}");
        if !existing.contains(candidate.as_str()) {
            let label_source = if use_title {
                node_title.unwrap_or(input_key)
            } else {
                input_key
            };
            return (candidate, friendly_provider_label(label_source));
        }
    }
    unreachable!()
}

pub(super) fn is_generic_provider_input_key(input_key: &str) -> bool {
    matches!(
        input_key.trim().to_ascii_lowercase().as_str(),
        "text" | "image" | "video" | "audio" | "value" | "filename" | "file"
    )
}

pub(super) fn sanitize_provider_input_name(value: &str) -> Option<String> {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator && !output.is_empty() {
            output.push('_');
            last_was_separator = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        None
    } else if output
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_digit())
    {
        Some(format!("input_{output}"))
    } else {
        Some(output)
    }
}

pub(super) fn infer_provider_input_from_workflow_node(
    node: &crate::core::comfyui_workflow::ComfyWorkflowNode,
    input_key: &str,
) -> (String, bool) {
    let key = input_key.to_ascii_lowercase();
    let class_type = node.class_type.to_ascii_lowercase();
    let title = node.title.as_deref().unwrap_or("").to_ascii_lowercase();
    if key.contains("image") || class_type.contains("loadimage") {
        return ("image".to_string(), false);
    }
    if key.contains("video") || class_type.contains("loadvideo") {
        return ("video".to_string(), false);
    }
    if key.contains("audio") || class_type.contains("loadaudio") {
        return ("audio".to_string(), false);
    }
    if key.contains("seed")
        || matches!(
            key.as_str(),
            "steps" | "width" | "height" | "batch_size" | "frames" | "frame_load_cap"
        )
    {
        return ("integer".to_string(), false);
    }
    if key.contains("cfg")
        || key.contains("denoise")
        || key.contains("duration")
        || key.contains("rate")
        || key.contains("crf")
    {
        return ("number".to_string(), false);
    }
    if class_type.contains("boolean") || matches!(key.as_str(), "enabled" | "save_output") {
        return ("boolean".to_string(), false);
    }
    let multiline =
        class_type.contains("multiline") || title.contains("prompt") || key.contains("prompt");
    ("text".to_string(), multiline)
}

pub(super) fn default_output_key(output_type: ProviderOutputType) -> &'static str {
    match output_type {
        ProviderOutputType::Image => "images",
        ProviderOutputType::Video => "images",
        ProviderOutputType::Audio => "audio",
    }
}

pub(super) fn inferred_output_key_for_node(
    node: &ProviderOutputNodeDraft,
    output_type: ProviderOutputType,
) -> String {
    let class_type = node.class_type.to_ascii_lowercase();
    if class_type.contains("savevideo") || class_type.contains("videocombine") {
        // ComfyUI video saver/combine nodes commonly report downloadable mp4s
        // under the historical `images` output key.
        return "images".to_string();
    }
    if class_type.contains("saveimage") {
        return "images".to_string();
    }
    if class_type.contains("saveaudio") || class_type.contains("audio") {
        return "audio".to_string();
    }
    default_output_key(output_type).to_string()
}

pub(super) fn empty_dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "-"
    } else {
        value
    }
}
