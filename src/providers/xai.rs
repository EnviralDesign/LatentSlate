use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::providers::{cloud, ProviderOutput, ProviderProgress};

const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";
const VIDEO_POLL_INTERVAL: Duration = Duration::from_secs(2);
const VIDEO_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub async fn generate_image(
    api_key: Option<&str>,
    model: &str,
    base_url: Option<&str>,
    inputs: &HashMap<String, Value>,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<ProviderOutput, String> {
    let api_key = cloud::required_api_key(api_key, "xAI")?;
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

pub async fn generate_video(
    api_key: Option<&str>,
    model: &str,
    base_url: Option<&str>,
    inputs: &HashMap<String, Value>,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<ProviderOutput, String> {
    let api_key = cloud::required_api_key(api_key, "xAI")?;
    let prompt = cloud::text_input(inputs, &["prompt", "positive_prompt"])
        .ok_or_else(|| "xAI video providers require a prompt input.".to_string())?;
    let duration = cloud::integer_input(inputs, "duration", 6).clamp(1, 15);
    let aspect_ratio = cloud::string_input(inputs, "aspect_ratio", "16:9");
    let resolution = cloud::string_input(inputs, "resolution", "480p");
    let base_url = base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/');

    let body = json!({
        "model": model,
        "prompt": prompt,
        "duration": duration,
        "aspect_ratio": aspect_ratio,
        "resolution": resolution,
    });

    let client = reqwest::Client::new();
    cloud::send_progress(&progress_tx, 0.02);
    let request_id = start_video_generation(&client, &api_key, base_url, &body).await?;
    cloud::send_progress(&progress_tx, 0.05);

    let video_url =
        poll_video_generation(&client, &api_key, base_url, &request_id, &progress_tx).await?;
    cloud::send_progress(&progress_tx, 0.96);

    let bytes = client
        .get(&video_url)
        .send()
        .await
        .map_err(|err| format!("xAI video download failed: {err}"))?
        .error_for_status()
        .map_err(|err| format!("xAI video download failed: {err}"))?
        .bytes()
        .await
        .map_err(|err| format!("xAI video bytes read failed: {err}"))?
        .to_vec();
    cloud::send_progress(&progress_tx, 1.0);

    Ok(ProviderOutput {
        bytes,
        extension: extension_from_url(&video_url).unwrap_or_else(|| "mp4".to_string()),
    })
}

async fn start_video_generation(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    body: &Value,
) -> Result<String, String> {
    let response = client
        .post(format!("{base_url}/videos/generations"))
        .bearer_auth(api_key)
        .json(body)
        .send()
        .await
        .map_err(|err| format!("xAI video request failed: {err}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("xAI video response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!("xAI video generation failed ({status}): {text}"));
    }
    let payload: Value =
        serde_json::from_str(&text).map_err(|err| format!("xAI returned invalid JSON: {err}"))?;
    payload
        .get("request_id")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "xAI video response did not include request_id.".to_string())
}

async fn poll_video_generation(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    request_id: &str,
    progress_tx: &Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<String, String> {
    let started = Instant::now();
    loop {
        if started.elapsed() > VIDEO_TIMEOUT {
            return Err("xAI video generation timed out.".to_string());
        }

        let response = client
            .get(format!("{base_url}/videos/{request_id}"))
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(|err| format!("xAI video poll failed: {err}"))?;
        let status_code = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| format!("xAI video poll response read failed: {err}"))?;
        if !status_code.is_success() {
            return Err(format!("xAI video poll failed ({status_code}): {text}"));
        }
        let payload: Value = serde_json::from_str(&text)
            .map_err(|err| format!("xAI video poll returned invalid JSON: {err}"))?;

        let progress = payload
            .get("progress")
            .and_then(|value| value.as_f64())
            .map(|value| (0.05 + (value.clamp(0.0, 100.0) as f32 / 100.0) * 0.9).min(0.95));
        if let Some(progress) = progress {
            cloud::send_progress(progress_tx, progress);
        }

        match payload
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
        {
            "done" => {
                return payload
                    .get("video")
                    .and_then(|video| video.get("url"))
                    .and_then(|url| url.as_str())
                    .map(str::to_string)
                    .ok_or_else(|| "xAI video result did not include video.url.".to_string());
            }
            "pending" | "" => tokio::time::sleep(VIDEO_POLL_INTERVAL).await,
            "expired" => return Err("xAI video request expired.".to_string()),
            "failed" => {
                return Err(format!(
                    "xAI video request failed: {}",
                    error_message(&payload)
                ));
            }
            other => {
                return Err(format!(
                    "xAI video request returned unexpected status '{other}'."
                ));
            }
        }
    }
}

fn error_message(payload: &Value) -> String {
    payload
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .or_else(|| error.get("details"))
                .and_then(|value| value.as_str())
        })
        .or_else(|| payload.get("message").and_then(|value| value.as_str()))
        .unwrap_or("no details")
        .to_string()
}

fn extension_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let extension = path.rsplit('.').next()?.to_ascii_lowercase();
    match extension.as_str() {
        "mp4" | "mov" | "mkv" | "webm" | "m4v" => Some(extension),
        _ => None,
    }
}
