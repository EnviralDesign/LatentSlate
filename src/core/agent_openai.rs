//! OpenAI account login and model discovery. Subscription credentials belong to this app.
use crate::state::{AgentConnection, AgentProviderEntry, OpenAiAuth};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{sync::mpsc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const API_BASE: &str = "https://api.openai.com/v1";
pub const CODEX_BASE: &str = "https://chatgpt.com/backend-api/codex";
static AUTH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub access_token: String,
    refresh_token: String,
    pub account_id: String,
    expires_at: i64,
    label: String,
}

#[derive(Clone)]
pub struct AgentModel {
    pub id: String,
    pub label: String,
    pub image: Option<bool>,
    pub video: Option<bool>,
    pub reasoning_efforts: Option<Vec<String>>,
    pub default_reasoning_effort: Option<String>,
}

pub enum SettingsAction {
    Login,
    Models,
    Logout,
}
pub enum SettingsEvent {
    Browser(String),
    Finished(Result<Option<Vec<AgentModel>>, String>),
}
pub struct SettingsRequest {
    pub events: mpsc::Receiver<SettingsEvent>,
    cancel: tokio::sync::watch::Sender<bool>,
}
impl Drop for SettingsRequest {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}

pub fn start_settings(provider: AgentProviderEntry, action: SettingsAction) -> SettingsRequest {
    let (send, events) = mpsc::channel();
    let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
    std::thread::spawn(move || {
        let result = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(async {
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => Err("Account operation canceled.".into()),
                    result = async {
                        match action {
                            SettingsAction::Login => { login(provider.id, &send).await?; Ok(None) }
                            SettingsAction::Models => models(&provider).await.map(Some),
                            SettingsAction::Logout => {
                                let _lock = AUTH_LOCK.lock().await;
                                forget_account(provider.id)?;
                                Ok(None)
                            }
                        }
                    } => result,
                }
            }),
            Err(_) => Err("Unable to start account worker.".into()),
        };
        let _ = send.send(SettingsEvent::Finished(result));
    });
    SettingsRequest { events, cancel }
}

fn account_path(id: Uuid) -> std::path::PathBuf {
    super::agent_provider_store::root()
        .join("accounts")
        .join(format!("{id}.bin"))
}

fn load_account(id: Uuid) -> Result<Account, String> {
    let bytes = std::fs::read(account_path(id)).map_err(|_| "Sign in with ChatGPT first.")?;
    let plain = protect(&bytes, false)?;
    serde_json::from_slice(&plain)
        .map_err(|_| "Saved ChatGPT login is unreadable. Sign in again.".into())
}

pub fn account_label(id: Uuid) -> String {
    match load_account(id) {
        Ok(account) => format!("Connected: {}", account.label),
        Err(error) => error,
    }
}

pub fn forget_account(id: Uuid) -> Result<(), String> {
    match std::fs::remove_file(account_path(id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Unable to remove saved ChatGPT login.".into()),
    }
}

fn save_account(id: Uuid, account: &Account) -> Result<(), String> {
    let bytes = serde_json::to_vec(account).map_err(|_| "Unable to encode ChatGPT login.")?;
    let bytes = protect(&bytes, true)?;
    let path = account_path(id);
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|_| "Unable to create account storage.")?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)
        .and_then(|_| std::fs::rename(&temporary, &path))
        .map_err(|_| "Unable to save ChatGPT login.".into())
}

#[cfg(windows)]
fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| "Credential too large.")?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI binds these credentials to the signed-in Windows user. The returned buffer is LocalAlloc-owned.
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("Windows could not access the encrypted ChatGPT login.".into());
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

#[cfg(not(windows))]
fn protect(_: &[u8], _: bool) -> Result<Vec<u8>, String> {
    Err(
        "ChatGPT credential storage currently requires Windows. Use an API key on this platform."
            .into(),
    )
}

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .user_agent("LatentSlate")
        .build()
        .map_err(|_| "Unable to initialize OpenAI connection.".into())
}

async fn token_request(
    fields: &[(&str, &str)],
    previous: Option<&Account>,
) -> Result<Account, String> {
    let response = client()?
        .post(TOKEN_URL)
        .form(fields)
        .send()
        .await
        .map_err(|_| "OpenAI sign-in connection failed. Try again.")?;
    if !response.status().is_success() {
        return Err(format!(
            "OpenAI sign-in failed (HTTP {}). Sign in again if the session expired.",
            response.status().as_u16()
        ));
    }
    let value = limited_json(response).await?;
    let access = value["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("OpenAI did not return an access token.")?;
    let claims = jwt_claims(access)?;
    let id_claims = value["id_token"]
        .as_str()
        .and_then(|s| jwt_claims(s).ok())
        .unwrap_or(Value::Null);
    let auth = &claims["https://api.openai.com/auth"];
    let account_id = auth["chatgpt_account_id"]
        .as_str()
        .or(id_claims["https://api.openai.com/auth"]["chatgpt_account_id"].as_str())
        .or(previous.map(|p| p.account_id.as_str()))
        .ok_or("OpenAI did not return an account ID.")?;
    let email = id_claims["email"]
        .as_str()
        .or(claims["https://api.openai.com/profile"]["email"].as_str());
    let plan = auth["chatgpt_plan_type"]
        .as_str()
        .or(id_claims["https://api.openai.com/auth"]["chatgpt_plan_type"].as_str());
    let label = match (email, plan) {
        (Some(email), Some(plan)) => format!("{email} · {plan}"),
        (Some(email), _) => email.to_owned(),
        (_, Some(plan)) => format!("ChatGPT · {plan}"),
        _ => previous
            .map(|p| p.label.clone())
            .unwrap_or_else(|| "ChatGPT account".into()),
    };
    Ok(Account {
        access_token: access.into(),
        refresh_token: value["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or(previous.map(|p| p.refresh_token.as_str()))
            .ok_or("OpenAI did not return a refresh token.")?
            .into(),
        account_id: account_id.into(),
        expires_at: value["expires_in"]
            .as_i64()
            .map(|s| chrono::Utc::now().timestamp() + s)
            .or(claims["exp"].as_i64())
            .ok_or("OpenAI did not return token expiry.")?,
        label,
    })
}

fn jwt_claims(token: &str) -> Result<Value, String> {
    // Claims are display/routing metadata from a token returned directly by OpenAI, not local authorization.
    let data = token.split('.').nth(1).ok_or("Invalid OpenAI token.")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(data)
        .map_err(|_| "Invalid OpenAI token encoding.")?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid OpenAI token claims.".into())
}

pub async fn account(id: Uuid) -> Result<Account, String> {
    let _lock = AUTH_LOCK.lock().await;
    let mut account = load_account(id)?;
    if account.expires_at <= chrono::Utc::now().timestamp() + 60 {
        account = token_request(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", &account.refresh_token),
            ],
            Some(&account),
        )
        .await?;
        save_account(id, &account)?;
    }
    Ok(account)
}

async fn login(id: Uuid, events: &mpsc::Sender<SettingsEvent>) -> Result<(), String> {
    // Bind before opening the browser, and never read another application's login cache.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:1455")
        .await
        .map_err(|_| {
            "Sign-in callback port 1455 is busy. Finish the other sign-in, then try again."
        })?;
    let verifier = format!(
        "{}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let state = Uuid::new_v4().simple().to_string();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse("https://auth.openai.com/oauth/authorize").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", REDIRECT_URI),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "latentslate"),
    ]);
    events
        .send(SettingsEvent::Browser(url.to_string()))
        .map_err(|_| "Sign-in closed.")?;
    let code = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let (mut socket, _) = listener.accept().await.map_err(|_| "Sign-in callback failed.")?;
            let mut line = String::new();
            let read = tokio::time::timeout(Duration::from_secs(3), BufReader::new((&mut socket).take(8192)).read_line(&mut line)).await;
            if !matches!(read, Ok(Ok(_))) { continue; }
            let parsed = line.strip_prefix("GET ").and_then(|s| s.split_whitespace().next())
                .and_then(|path| reqwest::Url::parse(&format!("http://localhost{path}")).ok());
            let query = parsed.as_ref().map(|u| u.query_pairs().into_owned().collect::<std::collections::HashMap<_, _>>()).unwrap_or_default();
            let valid = parsed.as_ref().is_some_and(|u| u.path() == "/auth/callback") && query.get("state") == Some(&state);
            let body = if valid { "Sign-in received. Return to LatentSlate to check the connection." } else { "Invalid sign-in callback. Return to LatentSlate." };
            let reply = format!("HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", if valid { "200 OK" } else { "400 Bad Request" }, body.len(), body);
            let _ = tokio::time::timeout(Duration::from_secs(2), socket.write_all(reply.as_bytes())).await;
            if !valid { continue; }
            if query.contains_key("error") { return Err("OpenAI sign-in was declined. Try again when ready.".to_string()); }
            if let Some(code) = query.get("code").filter(|s| !s.is_empty()) { return Ok(code.clone()); }
        }
    }).await.map_err(|_| "Sign-in timed out. Try again.")??;
    let _lock = AUTH_LOCK.lock().await;
    let account = token_request(
        &[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", &verifier),
        ],
        None,
    )
    .await?;
    save_account(id, &account)
}

pub async fn limited_json(mut response: reqwest::Response) -> Result<Value, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Provider response was interrupted.")?
    {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("Provider metadata exceeded the size limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Provider returned invalid JSON.".into())
}

pub fn endpoint(base: &str, path: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(&format!("{}/{}", base.trim().trim_end_matches('/'), path))
        .map_err(|_| "Invalid agent base URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Use an HTTP(S) base URL without credentials, query, or fragment.".into());
    }
    Ok(url)
}

async fn models(provider: &AgentProviderEntry) -> Result<Vec<AgentModel>, String> {
    let client = client()?;
    let mut request = match &provider.connection {
        AgentConnection::OpenAiCompatible {
            base_url, api_key, ..
        } => {
            let mut request = client.get(endpoint(base_url, "models")?);
            if let Some(key) = api_key.as_deref().filter(|k| !k.trim().is_empty()) {
                request = request.bearer_auth(key.trim());
            }
            request
        }
        AgentConnection::OpenAi {
            auth: OpenAiAuth::ApiKey,
            api_key,
            ..
        } => client.get(format!("{API_BASE}/models")).bearer_auth(
            api_key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or("Enter an OpenAI API key first.")?
                .trim(),
        ),
        AgentConnection::OpenAi { .. } => {
            let account = account(provider.id).await?;
            client
                .get(format!("{CODEX_BASE}/models"))
                .query(&[("client_version", "0.154.0")])
                .bearer_auth(account.access_token)
                .header("ChatGPT-Account-Id", account.account_id)
                .header("originator", "latentslate")
        }
    };
    request = request.header("Accept", "application/json");
    let response = request
        .send()
        .await
        .map_err(|_| "Could not load agent models.")?;
    if !response.status().is_success() {
        return Err(format!(
            "Model discovery failed (HTTP {}). Check the connection or enter a model ID manually.",
            response.status().as_u16()
        ));
    }
    let value = limited_json(response).await?;
    let entries = value["data"]
        .as_array()
        .or(value["models"].as_array())
        .ok_or("Provider returned no model list.")?;
    let mut result = Vec::new();
    for entry in entries {
        let Some(id) = entry["id"].as_str().or(entry["slug"].as_str()) else {
            continue;
        };
        if entry["visibility"].as_str() == Some("hide") {
            continue;
        }
        let modalities = entry["input_modalities"]
            .as_array()
            .or(entry["architecture"]["input_modalities"].as_array());
        let mut model = AgentModel {
            id: id.into(),
            label: entry["display_name"].as_str().unwrap_or(id).into(),
            reasoning_efforts: entry["supported_reasoning_levels"]
                .as_array()
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(|level| level["effort"].as_str().map(str::to_owned))
                        .collect()
                }),
            default_reasoning_effort: entry["default_reasoning_level"].as_str().map(str::to_owned),
            image: modalities.map(|m| m.iter().any(|v| v == "image")),
            video: if matches!(provider.connection, AgentConnection::OpenAi { .. }) {
                Some(false)
            } else {
                None
            },
        };
        // llama.cpp's model catalog omits video; only /props on an already-loaded selection supplies it.
        let selected = id == provider.connection.model()
            || entry["aliases"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == provider.connection.model()));
        if selected && entry["status"]["value"] == "loaded" {
            if let AgentConnection::OpenAiCompatible {
                base_url, api_key, ..
            } = &provider.connection
            {
                let mut url = endpoint(base_url, "models")?;
                url.set_path("/props");
                url.query_pairs_mut()
                    .append_pair("model", provider.connection.model());
                let mut request = client.get(url);
                if let Some(key) = api_key.as_deref().filter(|k| !k.trim().is_empty()) {
                    request = request.bearer_auth(key.trim());
                }
                if let Ok(response) = request.send().await {
                    if response.status().is_success() {
                        if let Ok(props) = limited_json(response).await {
                            model.image = props["modalities"]["vision"].as_bool().or(model.image);
                            model.video = props["modalities"]["video"].as_bool();
                        }
                    }
                }
            }
        }
        // Preserve a selected alias so model discovery doesn't unexpectedly change the user's model ID.
        if selected {
            model.id = provider.connection.model().into();
        }
        result.push(model);
    }
    result.sort_by(|a, b| a.label.cmp(&b.label));
    result.dedup_by(|a, b| a.id == b.id);
    Ok(result)
}
