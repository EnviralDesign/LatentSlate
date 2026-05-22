use std::collections::HashMap;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::state::{ProviderConnection, ProviderEntry, ProviderOutputType};

mod cloud;
pub mod comfyui;
pub mod openai;
pub mod xai;

#[derive(Debug, Clone)]
pub struct ProviderOutput {
    pub bytes: Vec<u8>,
    pub extension: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderProgress {
    pub overall: Option<f32>,
    pub node: Option<f32>,
}

impl ProviderProgress {
    pub fn overall(value: f32) -> Self {
        Self {
            overall: Some(value),
            node: None,
        }
    }

    pub fn node(value: f32) -> Self {
        Self {
            overall: None,
            node: Some(value),
        }
    }
}

#[derive(Debug)]
pub enum ProviderExecutionError {
    Offline(String),
    Error(String),
}

pub async fn execute_generation(
    provider: &ProviderEntry,
    inputs: &HashMap<String, Value>,
    output_type: ProviderOutputType,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<ProviderOutput, ProviderExecutionError> {
    match provider.connection.clone() {
        ProviderConnection::ComfyUi {
            base_url,
            workflow_path,
            manifest_path,
        } => {
            let workflow_path = comfyui::resolve_workflow_path(workflow_path.as_deref());
            let manifest_path = comfyui::resolve_manifest_path(manifest_path.as_deref());
            if let Err(err) = comfyui::check_health(&base_url).await {
                return Err(ProviderExecutionError::Offline(err));
            }
            match comfyui::generate_output(
                &base_url,
                &workflow_path,
                inputs,
                manifest_path.as_deref(),
                output_type,
                progress_tx,
            )
            .await
            {
                Ok(output) => Ok(ProviderOutput {
                    bytes: output.bytes,
                    extension: output.extension,
                }),
                Err(err) => {
                    if let Err(health_err) = comfyui::check_health(&base_url).await {
                        Err(ProviderExecutionError::Offline(health_err))
                    } else {
                        Err(ProviderExecutionError::Error(err))
                    }
                }
            }
        }
        ProviderConnection::OpenAiImage {
            credential_id,
            model,
            base_url,
        } => openai::generate_image(
            &credential_id,
            &model,
            base_url.as_deref(),
            inputs,
            progress_tx,
        )
        .await
        .map_err(ProviderExecutionError::Error),
        ProviderConnection::XaiImage {
            credential_id,
            model,
            base_url,
        } => xai::generate_image(
            &credential_id,
            &model,
            base_url.as_deref(),
            inputs,
            progress_tx,
        )
        .await
        .map_err(ProviderExecutionError::Error),
        ProviderConnection::XaiVideo {
            credential_id,
            model,
            base_url,
        } => xai::generate_video(
            &credential_id,
            &model,
            base_url.as_deref(),
            inputs,
            progress_tx,
        )
        .await
        .map_err(ProviderExecutionError::Error),
        ProviderConnection::CustomHttp { .. } => Err(ProviderExecutionError::Error(
            "Custom HTTP providers are planned but not implemented yet.".to_string(),
        )),
    }
}
