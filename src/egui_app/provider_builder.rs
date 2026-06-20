use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, Ui, Vec2};
use uuid::Uuid;

use crate::state::{
    ClipImageMode, ComfyOutputSelector, ComfyWorkflowRef, InputBinding, InputRole, InputUi,
    ManifestInput, NodeSelector, ProviderConnection, ProviderEntry, ProviderInputField,
    ProviderInputType, ProviderManifest, ProviderOutputType, ProviderWorkflowKind,
};
use crate::ui_kit as kit;

use super::{
    automation_checkbox, automation_selectable_value, inspector_numeric_field,
    inspector_numeric_rect, paint_truncated_row_text_bottom, paint_truncated_row_text_top,
    INSPECTOR_NUMERIC_H,
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
    Workflow,
    Settings,
    Output,
    Inputs,
}

impl ProviderBuilderTab {
    pub(super) const ALL: [ProviderBuilderTab; 4] = [
        ProviderBuilderTab::Workflow,
        ProviderBuilderTab::Settings,
        ProviderBuilderTab::Output,
        ProviderBuilderTab::Inputs,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            ProviderBuilderTab::Workflow => "Workflow",
            ProviderBuilderTab::Settings => "Settings",
            ProviderBuilderTab::Output => "Output",
            ProviderBuilderTab::Inputs => "Inputs",
        }
    }

    pub(super) fn step_number(self) -> usize {
        match self {
            ProviderBuilderTab::Workflow => 1,
            ProviderBuilderTab::Settings => 2,
            ProviderBuilderTab::Output => 3,
            ProviderBuilderTab::Inputs => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ProviderBuilderState {
    pub(super) source_path: Option<PathBuf>,
    pub(super) provider_id: Uuid,
    pub(super) provider_name: String,
    pub(super) provider_description: String,
    pub(super) output_type: ProviderOutputType,
    pub(super) workflow_kind: ProviderWorkflowKind,
    pub(super) workflow_kind_selected: bool,
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
    pub(super) selected_input_index: Option<usize>,
    pub(super) dragging_input_index: Option<usize>,
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
    pub(super) description: String,
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
    Select(usize),
    StartDrag(usize),
    StopDrag,
    Move { from: usize, to: usize },
    MoveUp(usize),
    MoveDown(usize),
    Delete(usize),
}

pub(super) struct ProviderInputSummaryRowResponse {
    pub(super) select_response: egui::Response,
    pub(super) drag_response: egui::Response,
    pub(super) rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProviderGenerationChoice {
    pub(super) workflow_kind: ProviderWorkflowKind,
    pub(super) output_type: ProviderOutputType,
}

impl ProviderGenerationChoice {
    pub(super) const ALL: [ProviderGenerationChoice; 11] = [
        ProviderGenerationChoice::new(ProviderWorkflowKind::TextToImage, ProviderOutputType::Image),
        ProviderGenerationChoice::new(
            ProviderWorkflowKind::ImageToImage,
            ProviderOutputType::Image,
        ),
        ProviderGenerationChoice::new(ProviderWorkflowKind::TextToVideo, ProviderOutputType::Video),
        ProviderGenerationChoice::new(
            ProviderWorkflowKind::ImageToVideo,
            ProviderOutputType::Video,
        ),
        ProviderGenerationChoice::new(
            ProviderWorkflowKind::FirstFrameLastFrameVideo,
            ProviderOutputType::Video,
        ),
        ProviderGenerationChoice::new(
            ProviderWorkflowKind::VideoToVideo,
            ProviderOutputType::Video,
        ),
        ProviderGenerationChoice::new(ProviderWorkflowKind::TextToAudio, ProviderOutputType::Audio),
        ProviderGenerationChoice::new(
            ProviderWorkflowKind::AudioToAudio,
            ProviderOutputType::Audio,
        ),
        ProviderGenerationChoice::new(ProviderWorkflowKind::Custom, ProviderOutputType::Image),
        ProviderGenerationChoice::new(ProviderWorkflowKind::Custom, ProviderOutputType::Video),
        ProviderGenerationChoice::new(ProviderWorkflowKind::Custom, ProviderOutputType::Audio),
    ];

    pub(super) const fn new(
        workflow_kind: ProviderWorkflowKind,
        output_type: ProviderOutputType,
    ) -> Self {
        Self {
            workflow_kind,
            output_type,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match (self.workflow_kind, self.output_type) {
            (ProviderWorkflowKind::Custom, ProviderOutputType::Image) => "Custom Image",
            (ProviderWorkflowKind::Custom, ProviderOutputType::Video) => "Custom Video",
            (ProviderWorkflowKind::Custom, ProviderOutputType::Audio) => "Custom Audio",
            _ => self.workflow_kind.label(),
        }
    }
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
        let is_existing_entry = source_path.is_some();
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
        let initial_tab = if workflow_path.is_some() {
            ProviderBuilderTab::Settings
        } else {
            ProviderBuilderTab::Workflow
        };

        let mut state = Self {
            source_path,
            provider_id: entry.id,
            provider_name: entry.name.clone(),
            provider_description: entry.description.clone().unwrap_or_default(),
            output_type: entry.output_type,
            workflow_kind: entry.workflow_kind,
            workflow_kind_selected: is_existing_entry
                && entry.workflow_kind != ProviderWorkflowKind::Auto,
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
            selected_input_index: None,
            dragging_input_index: None,
            tab: initial_tab,
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
        state.sync_output_type_from_generation();
        state.ensure_valid_selected_input();
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

    pub(super) fn ensure_valid_selected_input(&mut self) {
        let len = self.inputs.len();
        if len == 0 {
            self.selected_input_index = None;
            self.dragging_input_index = None;
            return;
        }
        if !self.selected_input_index.is_some_and(|index| index < len) {
            self.selected_input_index = Some(0);
        }
        if !self.dragging_input_index.is_some_and(|index| index < len) {
            self.dragging_input_index = None;
        }
    }

    pub(super) fn select_input(&mut self, index: usize) {
        if index < self.inputs.len() {
            self.selected_input_index = Some(index);
        }
    }

    pub(super) fn start_dragging_input(&mut self, index: usize) {
        if index < self.inputs.len() {
            self.selected_input_index = Some(index);
            self.dragging_input_index = Some(index);
        }
    }

    pub(super) fn stop_dragging_input(&mut self) {
        self.dragging_input_index = None;
    }

    pub(super) fn move_input(&mut self, from: usize, to: usize) {
        let len = self.inputs.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let input = self.inputs.remove(from);
        self.inputs.insert(to, input);
        self.selected_input_index = self.selected_input_index.map(|index| {
            remap_moved_index(index, from, to).min(self.inputs.len().saturating_sub(1))
        });
        self.dragging_input_index = self.dragging_input_index.map(|index| {
            remap_moved_index(index, from, to).min(self.inputs.len().saturating_sub(1))
        });
    }

    pub(super) fn delete_input(&mut self, index: usize) {
        if index >= self.inputs.len() {
            return;
        }
        self.inputs.remove(index);
        let len = self.inputs.len();
        self.selected_input_index = match (self.selected_input_index, len) {
            (_, 0) => None,
            (Some(selected), _) if selected == index => Some(index.min(len - 1)),
            (Some(selected), _) if selected > index => Some(selected - 1),
            (Some(selected), _) if selected < len => Some(selected),
            _ => Some(len - 1),
        };
        self.dragging_input_index = match (self.dragging_input_index, len) {
            (_, 0) => None,
            (Some(dragging), _) if dragging == index => None,
            (Some(dragging), _) if dragging > index => Some(dragging - 1),
            (Some(dragging), _) if dragging < len => Some(dragging),
            _ => None,
        };
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

    pub(super) fn workflow_selected(&self) -> bool {
        self.workflow_path.is_some()
    }

    pub(super) fn workflow_validation_error(&self) -> Option<String> {
        if !self.workflow_selected() {
            return Some("Choose a workflow JSON before continuing.".to_string());
        }
        if let Some(error) = &self.workflow_error {
            return Some(error.clone());
        }
        if self.workflow_nodes.is_empty() {
            return Some("The selected workflow did not expose any nodes.".to_string());
        }
        None
    }

    pub(super) fn settings_validation_error(&self) -> Option<String> {
        if let Some(error) = self.workflow_validation_error() {
            return Some(error);
        }
        if self.provider_name.trim().is_empty() {
            return Some("Provider name is required.".to_string());
        }
        if !self.workflow_kind_selected || self.workflow_kind == ProviderWorkflowKind::Auto {
            return Some("Choose the generation workflow.".to_string());
        }
        if self.base_url.trim().is_empty() {
            return Some("Base URL is required.".to_string());
        }
        None
    }

    pub(super) fn output_validation_error(&self) -> Option<String> {
        if let Some(error) = self.settings_validation_error() {
            return Some(error);
        }
        if !self.output_configured() {
            return Some("Select the workflow node that produces the final media.".to_string());
        }
        None
    }

    pub(super) fn inputs_validation_error(&self) -> Option<String> {
        if let Some(error) = self.output_validation_error() {
            return Some(error);
        }
        self.role_validation_error()
    }

    pub(super) fn current_step_error(&self) -> Option<String> {
        match self.tab {
            ProviderBuilderTab::Workflow => self.workflow_validation_error(),
            ProviderBuilderTab::Settings => self.settings_validation_error(),
            ProviderBuilderTab::Output => self.output_validation_error(),
            ProviderBuilderTab::Inputs => self.inputs_validation_error(),
        }
    }

    pub(super) fn save_validation_error(&self) -> Option<String> {
        self.inputs_validation_error()
    }

    pub(super) fn tab_available(&self, tab: ProviderBuilderTab) -> bool {
        match tab {
            ProviderBuilderTab::Workflow => true,
            ProviderBuilderTab::Settings => self.workflow_validation_error().is_none(),
            ProviderBuilderTab::Output => self.settings_validation_error().is_none(),
            ProviderBuilderTab::Inputs => self.output_validation_error().is_none(),
        }
    }

    pub(super) fn tab_unavailable_reason(&self, tab: ProviderBuilderTab) -> Option<String> {
        if self.tab_available(tab) {
            return None;
        }
        match tab {
            ProviderBuilderTab::Workflow => None,
            ProviderBuilderTab::Settings => self.workflow_validation_error(),
            ProviderBuilderTab::Output => self.settings_validation_error(),
            ProviderBuilderTab::Inputs => self.output_validation_error(),
        }
    }

    pub(super) fn next_tab(&self) -> Option<ProviderBuilderTab> {
        match self.tab {
            ProviderBuilderTab::Workflow => Some(ProviderBuilderTab::Settings),
            ProviderBuilderTab::Settings => Some(ProviderBuilderTab::Output),
            ProviderBuilderTab::Output => Some(ProviderBuilderTab::Inputs),
            ProviderBuilderTab::Inputs => None,
        }
    }

    pub(super) fn previous_tab(&self) -> Option<ProviderBuilderTab> {
        match self.tab {
            ProviderBuilderTab::Workflow => None,
            ProviderBuilderTab::Settings => Some(ProviderBuilderTab::Workflow),
            ProviderBuilderTab::Output => Some(ProviderBuilderTab::Settings),
            ProviderBuilderTab::Inputs => Some(ProviderBuilderTab::Output),
        }
    }

    pub(super) fn ensure_valid_tab(&mut self) {
        if self.workflow_validation_error().is_some() {
            self.tab = ProviderBuilderTab::Workflow;
            return;
        }
        if matches!(
            self.tab,
            ProviderBuilderTab::Output | ProviderBuilderTab::Inputs
        ) && self.settings_validation_error().is_some()
        {
            self.tab = ProviderBuilderTab::Settings;
            return;
        }
        if self.tab == ProviderBuilderTab::Inputs && self.output_validation_error().is_some() {
            self.tab = ProviderBuilderTab::Output;
        }
    }

    pub(super) fn preferred_edit_tab(&self) -> ProviderBuilderTab {
        if self.workflow_validation_error().is_some() {
            ProviderBuilderTab::Workflow
        } else if self.settings_validation_error().is_some() {
            ProviderBuilderTab::Settings
        } else if self.output_validation_error().is_some() {
            ProviderBuilderTab::Output
        } else {
            ProviderBuilderTab::Inputs
        }
    }

    pub(super) fn reset_workflow_bindings(&mut self) {
        self.output_node = None;
        self.output_key = default_output_key(self.output_type).to_string();
        self.output_tag.clear();
        self.inputs.clear();
        self.selected_input_index = None;
        self.dragging_input_index = None;
        self.tab = ProviderBuilderTab::Settings;
    }

    pub(super) fn apply_manifest(&mut self, manifest: ProviderManifest) {
        match manifest {
            ProviderManifest::ComfyUi {
                name,
                description,
                output_type,
                workflow,
                inputs,
                output,
                ..
            } => {
                if let Some(name) = name {
                    self.provider_name = name;
                }
                if let Some(description) = description {
                    self.provider_description = description;
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
                self.ensure_valid_selected_input();
            }
            ProviderManifest::CustomHttp {
                name,
                description,
                output_type,
                inputs,
                ..
            } => {
                if let Some(name) = name {
                    self.provider_name = name;
                }
                if let Some(description) = description {
                    self.provider_description = description;
                }
                self.output_type = output_type;
                self.inputs = inputs
                    .into_iter()
                    .map(ProviderBuilderInput::from_custom_http_input)
                    .collect();
                self.ensure_valid_selected_input();
                self.error = Some(
                    "Loaded a Custom HTTP manifest. Saving from this builder writes ComfyUI settings."
                        .to_string(),
                );
            }
        }
    }

    pub(super) fn generation_choice(&self) -> Option<ProviderGenerationChoice> {
        if !self.workflow_kind_selected || self.workflow_kind == ProviderWorkflowKind::Auto {
            return None;
        }
        ProviderGenerationChoice::ALL
            .iter()
            .copied()
            .find(|choice| {
                choice.workflow_kind == self.workflow_kind && choice.output_type == self.output_type
            })
            .or_else(|| {
                derived_output_type_for_workflow_kind(self.workflow_kind).map(|output_type| {
                    ProviderGenerationChoice::new(self.workflow_kind, output_type)
                })
            })
    }

    pub(super) fn apply_generation_choice(&mut self, choice: ProviderGenerationChoice) {
        let previous_output_type = self.output_type;
        self.workflow_kind = choice.workflow_kind;
        self.workflow_kind_selected = true;
        self.output_type = choice.output_type;
        if self.output_type != previous_output_type {
            self.output_node = None;
            self.output_key = default_output_key(self.output_type).to_string();
            self.output_tag.clear();
        }
    }

    pub(super) fn sync_output_type_from_generation(&mut self) {
        if !self.workflow_kind_selected {
            return;
        }
        if let Some(output_type) = derived_output_type_for_workflow_kind(self.workflow_kind) {
            self.output_type = output_type;
            self.output_key = default_output_key(self.output_type).to_string();
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

    pub(super) fn role_validation_error(&self) -> Option<String> {
        if !self.workflow_selected() {
            return None;
        }

        let mut role_inputs: HashMap<InputRole, Vec<String>> = HashMap::new();
        let mut invalid_type_inputs = Vec::new();

        for input in &self.inputs {
            let Some(role) = input.role else {
                continue;
            };
            role_inputs
                .entry(role)
                .or_default()
                .push(input.name.clone());
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
        for role in self.required_input_roles() {
            let names = role_inputs
                .get(&role)
                .map_or(&[][..], |inputs| inputs.as_slice());
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
            Some(format!(
                "Missing required roles: {}.",
                missing_roles.join(", ")
            ))
        } else {
            None
        }
    }

    pub(super) fn required_input_roles(&self) -> Vec<InputRole> {
        if self.output_type == ProviderOutputType::Audio {
            vec![InputRole::Seed]
        } else {
            vec![InputRole::Width, InputRole::Height, InputRole::Seed]
        }
    }

    pub(super) fn role_input_name(&self, role: InputRole) -> Option<&str> {
        self.inputs
            .iter()
            .find(|input| input.role == Some(role))
            .map(|input| input.name.as_str())
    }

    pub(super) fn satisfied_input_roles(&self) -> HashSet<InputRole> {
        self.required_input_roles()
            .into_iter()
            .filter(|role| {
                let matching: Vec<&ProviderBuilderInput> = self
                    .inputs
                    .iter()
                    .filter(|input| input.role == Some(*role))
                    .collect();
                matching.len() == 1 && is_numeric_type_value(&matching[0].input_type_key)
            })
            .collect()
    }

    pub(super) fn build_save_payload(&self) -> Result<ProviderBuilderSave, String> {
        if let Some(error) = self.save_validation_error() {
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
        let provider_description = optional_trimmed_string(&self.provider_description);
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
                description: optional_trimmed_string(&input.description),
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
                description: optional_trimmed_string(&input.description),
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
                description: provider_description.clone(),
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
            description: provider_description.clone(),
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
            description: provider_description,
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
            description: String::new(),
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
            description: input.description.clone().unwrap_or_default(),
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
            description: input.description.unwrap_or_default(),
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
            description: input.description.unwrap_or_default(),
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
    pub(super) description: Option<String>,
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
    let response = kit::draw_accent_row(ui, 52.0, selected, accent, |ui, rect| {
        paint_text_button_row(ui, rect, &summary.name, &summary.subtitle);
    });
    if let Some(description) = summary
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        response.on_hover_text(description)
    } else {
        response
    }
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
            description: None,
            output_type: None,
        };
    };
    let Ok(entry) = serde_json::from_str::<ProviderEntry>(&text) else {
        return ProviderFileSummary {
            name: file_name,
            subtitle: "Invalid provider JSON".to_string(),
            description: None,
            output_type: None,
        };
    };
    ProviderFileSummary {
        name: entry.name,
        subtitle: provider_output_type_label(entry.output_type).to_string(),
        description: entry.description,
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

pub(super) fn derived_output_type_for_workflow_kind(
    workflow_kind: ProviderWorkflowKind,
) -> Option<ProviderOutputType> {
    match workflow_kind {
        ProviderWorkflowKind::TextToImage | ProviderWorkflowKind::ImageToImage => {
            Some(ProviderOutputType::Image)
        }
        ProviderWorkflowKind::TextToVideo
        | ProviderWorkflowKind::ImageToVideo
        | ProviderWorkflowKind::FirstFrameLastFrameVideo
        | ProviderWorkflowKind::VideoToVideo => Some(ProviderOutputType::Video),
        ProviderWorkflowKind::TextToAudio | ProviderWorkflowKind::AudioToAudio => {
            Some(ProviderOutputType::Audio)
        }
        ProviderWorkflowKind::Auto | ProviderWorkflowKind::Custom => None,
    }
}

pub(super) fn clip_image_mode_label(mode: ClipImageMode) -> &'static str {
    match mode {
        ClipImageMode::Still => "Still Image",
        ClipImageMode::Keyframe => "Keyframe Reference",
    }
}

pub(super) fn provider_output_type_readout(
    ui: &mut Ui,
    label: &str,
    value: Option<ProviderOutputType>,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        let value = value
            .map(provider_output_type_label)
            .unwrap_or("Derived from generation");
        kit::readonly_value_box(
            ui,
            value,
            Vec2::new(ui.available_width(), kit::VALUE_FIELD_H),
        );
    });
}

pub(super) fn provider_workflow_kind_field(
    ui: &mut Ui,
    label: &str,
    selected: Option<ProviderGenerationChoice>,
) -> Option<ProviderGenerationChoice> {
    let selected_text = selected.map_or("Choose generation...", ProviderGenerationChoice::label);
    let mut selected_choice = selected.unwrap_or(ProviderGenerationChoice::ALL[0]);
    let mut next_choice = None;
    kit::labeled_combo_field(ui, label, "provider_workflow_kind", selected_text, |ui| {
        for choice in ProviderGenerationChoice::ALL {
            if automation_selectable_value(ui, &mut selected_choice, choice, choice.label())
                .clicked()
            {
                next_choice = Some(choice);
            }
        }
    });
    next_choice
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

pub(super) fn provider_builder_input_summary_row(
    ui: &mut Ui,
    index: usize,
    input: &ProviderBuilderInput,
    selected: bool,
    dragging: bool,
    role_satisfied: bool,
) -> ProviderInputSummaryRowResponse {
    let selection_accent = kit::BORDER_FOCUS;
    let row_h = 46.0;
    let (rect, base_response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), egui::Sense::hover());
    let id = ui.make_persistent_id(("provider_builder_input_summary_row", index));
    let handle_w = 34.0;
    let handle_rect = Rect::from_min_max(
        rect.left_top(),
        Pos2::new((rect.left() + handle_w).min(rect.right()), rect.bottom()),
    );
    let select_rect = Rect::from_min_max(
        Pos2::new(handle_rect.right(), rect.top()),
        rect.right_bottom(),
    );
    let raw_drag_response =
        ui.interact(handle_rect, id.with("drag"), egui::Sense::click_and_drag());
    let select_response = ui
        .interact(select_rect, id.with("select"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let drag_response = if dragging {
        raw_drag_response.on_hover_and_drag_cursor(egui::CursorIcon::Grabbing)
    } else {
        raw_drag_response.on_hover_and_drag_cursor(egui::CursorIcon::Grab)
    };
    let select_response =
        crate::core::automation::instrument_response(select_response, "row", None, true, false);
    let drag_response = crate::core::automation::instrument_response(
        drag_response,
        "drag_handle",
        None,
        true,
        false,
    );
    let hovered =
        base_response.hovered() || select_response.hovered() || drag_response.hovered() || dragging;
    let fill = kit::row_fill(selected, hovered);
    let stroke_color = if selected {
        selection_accent
    } else {
        kit::BORDER_SOFT
    };

    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(5), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(5),
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), Vec2::new(4.0, rect.height())),
        egui::CornerRadius::same(2),
        selection_accent,
    );

    let role_label = provider_input_compact_role_label(input.role);
    let role_color = input
        .role
        .filter(|_| role_satisfied)
        .map(provider_input_role_color);
    let role_w = 70.0;
    let role_rect = Rect::from_center_size(
        Pos2::new(rect.right() - role_w * 0.5 - 10.0, rect.center().y),
        Vec2::new(role_w, 24.0),
    );
    let role_fill = role_color
        .map(|color| color.gamma_multiply(0.18))
        .unwrap_or(kit::FIELD_BG_ACTIVE);
    let role_stroke = role_color
        .map(|color| color.gamma_multiply(0.72))
        .unwrap_or(kit::BORDER);
    let role_text = role_color.unwrap_or(if selected { kit::TEXT } else { kit::TEXT_MUTED });
    ui.painter()
        .rect_filled(role_rect, egui::CornerRadius::same(5), role_fill);
    ui.painter().rect_stroke(
        role_rect,
        egui::CornerRadius::same(5),
        egui::Stroke::new(1.0, role_stroke),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        role_rect.center(),
        egui::Align2::CENTER_CENTER,
        role_label,
        egui::FontId::proportional(11.0),
        role_text,
    );

    ui.painter().text(
        Pos2::new(rect.left() + 17.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        "::",
        egui::FontId::proportional(13.0),
        kit::TEXT_DIM,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 52.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        format!("{:02}", index + 1),
        egui::FontId::proportional(11.0),
        kit::TEXT_DIM,
    );

    let title = if input.label.trim().is_empty() {
        input.name.as_str()
    } else {
        input.label.as_str()
    };
    let subtitle = provider_input_compact_subtitle(input);
    let text_left = rect.left() + 72.0;
    let text_width = (role_rect.left() - text_left - 8.0).max(24.0);
    paint_truncated_row_text_top(
        ui,
        Pos2::new(text_left, rect.top() + 8.0),
        kit::value(title),
        12.0,
        text_width,
        kit::TEXT,
    );
    paint_truncated_row_text_bottom(
        ui,
        Pos2::new(text_left, rect.bottom() - 7.0),
        kit::caption(subtitle),
        11.0,
        text_width,
        kit::TEXT_MUTED,
    );

    ProviderInputSummaryRowResponse {
        select_response,
        drag_response,
        rect,
    }
}

pub(super) fn provider_builder_input_inspector_editor(
    ui: &mut Ui,
    index: usize,
    len: usize,
    input: &mut ProviderBuilderInput,
    action: &mut Option<ProviderInputAction>,
) {
    kit::labeled_text_field(ui, "Name", &mut input.name);
    ui.add_space(kit::FORM_ROW_GAP);
    kit::labeled_text_field(ui, "Label", &mut input.label);
    ui.add_space(kit::FORM_ROW_GAP);
    kit::field_label(ui, "Description");
    ui.add_space(kit::FIELD_LABEL_GAP);
    kit::multiline_text_field(
        ui,
        &mut input.description,
        ui.available_width(),
        kit::MultilineTextFieldOptions::rows(3),
    );
    ui.add_space(kit::FORM_ROW_GAP);
    provider_input_type_field(ui, "Type", &mut input.input_type_key);
    ui.add_space(kit::FORM_ROW_GAP);
    provider_builder_default_field(ui, input);
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
    if input.input_type_key == "text" {
        automation_checkbox(ui, &mut input.multiline, "Multiline text");
        ui.add_space(kit::FORM_ROW_GAP);
    } else {
        input.multiline = false;
    }
    provider_builder_input_action_row(ui, index, len, input, action);
}

pub(super) fn provider_builder_input_action_row(
    ui: &mut Ui,
    index: usize,
    len: usize,
    input: &ProviderBuilderInput,
    action: &mut Option<ProviderInputAction>,
) {
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let buttons_w = 42.0 + 52.0 + 66.0 + gap * 3.0;
        let binding_label = if input.required {
            format!(
                "node {} / {}.{}",
                input.selector.node_id.as_deref().unwrap_or("-"),
                empty_dash(&input.selector.class_type),
                empty_dash(&input.selector.input_key)
            )
        } else {
            format!(
                "Optional -> node {} / {}.{}",
                input.selector.node_id.as_deref().unwrap_or("-"),
                empty_dash(&input.selector.class_type),
                empty_dash(&input.selector.input_key)
            )
        };
        ui.add_sized(
            [(ui.available_width() - buttons_w).max(0.0), 18.0],
            egui::Label::new(kit::caption(binding_label)).truncate(),
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

pub(super) fn provider_input_role_color(role: InputRole) -> Color32 {
    match role {
        InputRole::Width => kit::IMAGE,
        InputRole::Height => kit::VIDEO,
        InputRole::Seed => kit::MARKER,
    }
}

pub(super) fn provider_input_compact_role_label(value: Option<InputRole>) -> &'static str {
    match value {
        Some(role) => provider_input_role_label(Some(role)),
        None => "Generic",
    }
}

fn provider_input_compact_subtitle(input: &ProviderBuilderInput) -> String {
    let default = provider_input_compact_default(input);
    let binding = format!(
        "node {} / {}.{}",
        input.selector.node_id.as_deref().unwrap_or("-"),
        empty_dash(&input.selector.class_type),
        empty_dash(&input.selector.input_key)
    );
    if input.required {
        format!(
            "{}  {}  {}",
            provider_input_type_label(&input.input_type_key),
            default,
            binding
        )
    } else {
        format!(
            "{}  {}  Optional -> {}",
            provider_input_type_label(&input.input_type_key),
            default,
            binding
        )
    }
}

fn provider_input_compact_default(input: &ProviderBuilderInput) -> String {
    match input.input_type_key.as_str() {
        "image" | "video" | "audio" => "Runtime asset".to_string(),
        _ => {
            let trimmed = input.default_text.trim();
            if trimmed.is_empty() {
                "No default".to_string()
            } else {
                format!("Default {}", trimmed)
            }
        }
    }
}

fn remap_moved_index(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < to && index > from && index <= to {
        index - 1
    } else if to < from && index >= to && index < from {
        index + 1
    } else {
        index
    }
}

pub(super) fn provider_input_role_field(
    ui: &mut Ui,
    label: &str,
    _input_name: &str,
    role: &mut Option<InputRole>,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = kit::FIELD_LABEL_GAP;
        kit::field_label(ui, label);
        let row_w = ui.available_width().max(0.0);
        let gap = kit::MEDIA_PILL_MIN_GAP;
        let button_w = ((row_w - gap * 3.0) / 4.0).max(42.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            if kit::timeline_tool_text_button(ui, "Generic", button_w, role.is_none()).clicked() {
                *role = None;
            }
            if kit::timeline_tool_text_button(
                ui,
                "Width",
                button_w,
                *role == Some(InputRole::Width),
            )
            .clicked()
            {
                *role = Some(InputRole::Width);
            }
            if kit::timeline_tool_text_button(
                ui,
                "Height",
                button_w,
                *role == Some(InputRole::Height),
            )
            .clicked()
            {
                *role = Some(InputRole::Height);
            }
            if kit::timeline_tool_text_button(ui, "Seed", button_w, *role == Some(InputRole::Seed))
                .clicked()
            {
                *role = Some(InputRole::Seed);
            }
        });
    });
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

pub(super) fn optional_trimmed_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
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
