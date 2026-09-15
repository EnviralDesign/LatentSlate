use std::collections::HashMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::providers::{cloud, ProviderOutput, ProviderProgress};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub async fn generate_image(
    api_key: Option<&str>,
    model: &str,
    base_url: Option<&str>,
    inputs: &HashMap<String, Value>,
    edit: bool,
    progress_tx: Option<mpsc::UnboundedSender<ProviderProgress>>,
) -> Result<ProviderOutput, String> {
    let api_key = cloud::required_api_key(api_key, "OpenAI")?;
    let prompt = cloud::text_input(inputs, &["prompt", "positive_prompt"])
        .ok_or_else(|| "OpenAI image providers require a prompt input.".to_string())?;
    let size = cloud::string_input(inputs, "size", "1024x1024");
    let quality = cloud::string_input(inputs, "quality", "auto");
    let output_format = cloud::string_input(inputs, "output_format", "png");
    let model = cloud::string_input(inputs, "model", model);
    let background = cloud::string_input(inputs, "background", "auto");
    if background == "transparent" && output_format == "jpeg" {
        return Err("Transparent backgrounds require PNG or WebP output.".to_string());
    }

    let body = json!({
        "model": model,
        "prompt": prompt,
        "size": size,
        "quality": quality,
        "output_format": output_format,
        "background": background,
    });

    cloud::send_progress(&progress_tx, 0.05);
    let client = reqwest::Client::new();
    let base_url = base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/');
    let request = if edit {
        let path = cloud::text_input(inputs, &["image"])
            .ok_or_else(|| "OpenAI image editing requires a reference image.".to_string())?;
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|err| format!("Could not read the reference image: {err}"))?;
        let mime = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Png) => "image/png",
            Ok(image::ImageFormat::Jpeg) => "image/jpeg",
            Ok(image::ImageFormat::WebP) => "image/webp",
            _ => return Err("OpenAI reference images must be PNG, JPEG, or WebP.".to_string()),
        };
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("reference.png")
            .to_string();
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(name)
            .mime_str(mime)
            .map_err(|err| err.to_string())?;
        let mut form = reqwest::multipart::Form::new().part("image[]", part);
        for (key, value) in body.as_object().unwrap() {
            form = form.text(key.clone(), value.as_str().unwrap().to_string());
        }
        client
            .post(format!("{base_url}/images/edits"))
            .multipart(form)
    } else {
        client
            .post(format!("{base_url}/images/generations"))
            .json(&body)
    };
    let response = request
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|err| format!("OpenAI image request failed: {err}"))?;
    cloud::send_progress(&progress_tx, 0.9);
    let output = cloud::parse_image_response(&client, "OpenAI", response, &output_format).await?;
    cloud::send_progress(&progress_tx, 1.0);
    Ok(output)
}
