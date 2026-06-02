//! App-managed credential storage.
//!
//! On Windows, secrets are protected with DPAPI before being written to the
//! repo-local ignored runtime folder. Provider JSON stores stable credential
//! IDs, not the API key material itself.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use base64::prelude::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const OPENAI_CREDENTIAL_ID: &str = "openai_api_key";
pub const XAI_CREDENTIAL_ID: &str = "xai_api_key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredential {
    pub label: String,
    pub protected_value: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CredentialFile {
    schema_version: u32,
    #[serde(default)]
    credentials: HashMap<String, StoredCredential>,
}

impl Default for CredentialFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            credentials: HashMap::new(),
        }
    }
}

pub fn credentials_path() -> PathBuf {
    crate::core::paths::app_runtime_root()
        .join("secrets")
        .join("credentials.json")
}

pub fn has_secret(id: &str) -> bool {
    load_file()
        .map(|file| file.credentials.contains_key(id))
        .unwrap_or(false)
}

pub fn save_secret(id: &str, label: &str, secret: &str) -> Result<(), String> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err("API key is empty.".to_string());
    }
    let encrypted = protect_secret(trimmed.as_bytes())?;
    let mut file = load_file().unwrap_or_default();
    file.credentials.insert(
        id.to_string(),
        StoredCredential {
            label: label.to_string(),
            protected_value: BASE64_STANDARD.encode(encrypted),
            updated_at: Utc::now(),
        },
    );
    save_file(&file).map_err(|err| format!("Failed to save credentials: {err}"))
}

pub fn delete_secret(id: &str) -> Result<(), String> {
    let mut file = load_file().unwrap_or_default();
    file.credentials.remove(id);
    save_file(&file).map_err(|err| format!("Failed to save credentials: {err}"))
}

pub fn load_secret(id: &str) -> Result<String, String> {
    let file = load_file().map_err(|err| format!("Failed to load credentials: {err}"))?;
    let record = file
        .credentials
        .get(id)
        .ok_or_else(|| format!("Missing API key for {id}. Add it in Settings > API Keys."))?;
    let encrypted = BASE64_STANDARD
        .decode(record.protected_value.as_bytes())
        .map_err(|err| format!("Credential record is not valid base64: {err}"))?;
    let bytes = unprotect_secret(&encrypted)?;
    String::from_utf8(bytes).map_err(|err| format!("Credential value is not UTF-8: {err}"))
}

pub fn secret_char_count(id: &str) -> Result<usize, String> {
    load_secret(id).map(|secret| secret.chars().count())
}

fn load_file() -> io::Result<CredentialFile> {
    let path = credentials_path();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(CredentialFile::default());
        }
        Err(err) => return Err(err),
    };
    serde_json::from_str(&text).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn save_file(file: &CredentialFile) -> io::Result<()> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(file)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    fs::write(path, text)
}

#[cfg(windows)]
fn protect_secret(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let entropy = credential_entropy();
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &input,
            null(),
            &entropy_blob,
            null_mut(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Windows DPAPI failed to protect the credential.".to_string());
    }

    let protected = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let value = slice.to_vec();
        LocalFree(output.pbData as _);
        value
    };
    Ok(protected)
}

#[cfg(windows)]
fn unprotect_secret(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let entropy = credential_entropy();
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            null_mut(),
            &entropy_blob,
            null_mut(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Windows DPAPI failed to unlock the credential.".to_string());
    }

    let secret = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let value = slice.to_vec();
        LocalFree(output.pbData as _);
        value
    };
    Ok(secret)
}

#[cfg(not(windows))]
fn protect_secret(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("Encrypted app credentials are only implemented on Windows for now.".to_string())
}

#[cfg(not(windows))]
fn unprotect_secret(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("Encrypted app credentials are only implemented on Windows for now.".to_string())
}

fn credential_entropy() -> &'static [u8] {
    b"EnviralDesign LatentSlate credential store v1"
}
