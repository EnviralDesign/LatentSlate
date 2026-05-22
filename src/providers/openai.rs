use std::collections::HashMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::core::credentials;
use crate::providers::{cloud, ProviderOutput, ProviderProgress};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub async fn generate_image(
    credential_id: &str,
    model: &str,
    base_url: Option<&str>,
    inputs: &HashMap<String, Value>,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<ProviderOutput, String> {
    let api_key = credentials::load_secret(credential_id)?;
    let prompt = cloud::text_input(inputs, &["prompt", "positive_prompt"])
        .ok_or_else(|| "OpenAI image providers require a prompt input.".to_string())?;
    let size = cloud::string_input(inputs, "size", "1024x1024");
    let quality = cloud::string_input(inputs, "quality", "auto");
    let output_format = cloud::string_input(inputs, "output_format", "png");

    let body = json!({
        "model": model,
        "prompt": prompt,
        "size": size,
        "quality": quality,
        "output_format": output_format,
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
        .map_err(|err| format!("OpenAI image request failed: {err}"))?;
    cloud::send_progress(&progress_tx, 0.9);
    let output = cloud::parse_image_response(&client, "OpenAI", response, &output_format).await?;
    cloud::send_progress(&progress_tx, 1.0);
    Ok(output)
}
