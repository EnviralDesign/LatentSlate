use std::collections::HashMap;

use base64::prelude::*;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::providers::{ProviderOutput, ProviderProgress};

pub fn text_input<'a>(inputs: &'a HashMap<String, Value>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        inputs
            .get(*name)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

pub fn string_input(inputs: &HashMap<String, Value>, name: &str, fallback: &str) -> String {
    inputs
        .get(name)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

pub fn integer_input(inputs: &HashMap<String, Value>, name: &str, fallback: i64) -> i64 {
    inputs
        .get(name)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().map(|value| value as i64))
                .or_else(|| value.as_f64().map(|value| value.round() as i64))
                .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
        })
        .unwrap_or(fallback)
}

pub fn send_progress(tx: &Option<mpsc::UnboundedSender<ProviderProgress>>, value: f32) {
    if let Some(tx) = tx {
        let _ = tx.send(ProviderProgress::overall(value.clamp(0.0, 1.0)));
    }
}

pub fn required_api_key<'a>(
    api_key: Option<&'a str>,
    provider_name: &str,
) -> Result<&'a str, String> {
    api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{provider_name} provider JSON is missing connection.api_key."))
}

pub async fn parse_image_response(
    client: &reqwest::Client,
    provider_name: &str,
    response: reqwest::Response,
    fallback_extension: &str,
) -> Result<ProviderOutput, String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("{provider_name} response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "{provider_name} image generation failed ({status}): {text}"
        ));
    }
    let payload: Value = serde_json::from_str(&text)
        .map_err(|err| format!("{provider_name} returned invalid JSON: {err}"))?;
    let Some(item) = payload
        .get("data")
        .and_then(|data| data.as_array())
        .and_then(|data| data.first())
    else {
        return Err(format!(
            "{provider_name} response did not include image data."
        ));
    };

    if let Some(encoded) = item.get("b64_json").and_then(|value| value.as_str()) {
        let bytes = BASE64_STANDARD
            .decode(encoded.as_bytes())
            .map_err(|err| format!("{provider_name} returned invalid image base64: {err}"))?;
        let extension = detect_image_extension(&bytes).unwrap_or_else(|| fallback_extension.into());
        return Ok(ProviderOutput { bytes, extension });
    }

    if let Some(url) = item.get("url").and_then(|value| value.as_str()) {
        let bytes = client
            .get(url)
            .send()
            .await
            .map_err(|err| format!("{provider_name} image download failed: {err}"))?
            .error_for_status()
            .map_err(|err| format!("{provider_name} image download failed: {err}"))?
            .bytes()
            .await
            .map_err(|err| format!("{provider_name} image bytes read failed: {err}"))?
            .to_vec();
        let extension = detect_image_extension(&bytes).unwrap_or_else(|| fallback_extension.into());
        return Ok(ProviderOutput { bytes, extension });
    }

    Err(format!(
        "{provider_name} response did not include b64_json or url image output."
    ))
}

fn detect_image_extension(bytes: &[u8]) -> Option<String> {
    let format = image::guess_format(bytes).ok()?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::Bmp => "bmp",
        image::ImageFormat::Tiff => "tiff",
        _ => return None,
    };
    Some(extension.to_string())
}
