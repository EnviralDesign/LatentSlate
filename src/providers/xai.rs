use std::collections::HashMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::core::credentials;
use crate::providers::{cloud, ProviderOutput, ProviderProgress};

const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";

pub async fn generate_image(
    credential_id: &str,
    model: &str,
    base_url: Option<&str>,
    inputs: &HashMap<String, Value>,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<ProviderOutput, String> {
    let api_key = credentials::load_secret(credential_id)?;
    let prompt = cloud::text_input(inputs, &["prompt", "positive_prompt"])
        .ok_or_else(|| "xAI image providers require a prompt input.".to_string())?;
    let aspect_ratio = cloud::string_input(inputs, "aspect_ratio", "1:1");
    let resolution = cloud::string_input(inputs, "resolution", "1k");

    let body = json!({
        "model": model,
        "prompt": prompt,
        "response_format": "b64_json",
        "aspect_ratio": aspect_ratio,
        "resolution": resolution,
    });

    cloud::send_progress(&progress_tx, 0.05);
    let client = reqwest::Client::new();
    let url = format!(
        "{}/images/generations",
        base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/')
    );
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("xAI image request failed: {err}"))?;
    cloud::send_progress(&progress_tx, 0.9);
    let output = cloud::parse_image_response(&client, "xAI", response, "jpg").await?;
    cloud::send_progress(&progress_tx, 1.0);
    Ok(output)
}
