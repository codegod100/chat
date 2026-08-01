//! Minimal OpenBao / Vault-compatible client for reading AI API keys.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

const DEFAULT_ADDR: &str = "https://openbao.boxd.sh";
const KEYS_PATH: &str = "secret/data/ai-api-keys";

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    token: String,
    http: reqwest::blocking::Client,
}

#[derive(Debug)]
pub enum BaoError {
    Http(reqwest::Error),
    Status { code: u16, message: String },
    Parse(String),
    NoToken,
}

impl std::fmt::Display for BaoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaoError::Http(e) => write!(f, "network error: {e}"),
            BaoError::Status { code, message } => write!(f, "HTTP {code}: {message}"),
            BaoError::Parse(e) => write!(f, "parse error: {e}"),
            BaoError::NoToken => write!(
                f,
                "no OpenBao token (set BAO_TOKEN / VAULT_TOKEN or ~/.bao-token)"
            ),
        }
    }
}

impl From<reqwest::Error> for BaoError {
    fn from(e: reqwest::Error) -> Self {
        BaoError::Http(e)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub initialized: bool,
    pub sealed: bool,
}

impl Client {
    pub fn new(address: &str, token: &str) -> Result<Self, BaoError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            base_url: address.trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
            http,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/v1/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn send(&self, builder: reqwest::blocking::RequestBuilder) -> Result<Value, BaoError> {
        let response = builder.send()?;
        let status = response.status();
        let body = response.text().unwrap_or_default();

        if !status.is_success() {
            let message = extract_error_message(&body).unwrap_or_else(|| {
                if body.is_empty() {
                    status.canonical_reason().unwrap_or("error").to_string()
                } else {
                    body.chars().take(300).collect()
                }
            });
            return Err(BaoError::Status {
                code: status.as_u16(),
                message,
            });
        }

        if body.trim().is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_str(&body).map_err(|e| BaoError::Parse(e.to_string()))
    }

    pub fn health(&self) -> Result<Health, BaoError> {
        let response = self
            .http
            .get(self.url("sys/health"))
            .header("X-Vault-Token", &self.token)
            .query(&[
                ("standbyok", "true"),
                ("sealedcode", "200"),
                ("uninitcode", "200"),
            ])
            .send()?;
        let body = response.text().unwrap_or_default();
        serde_json::from_str(&body).map_err(|e| BaoError::Parse(format!("{e}: {body}")))
    }

    /// Read KV v2 `secret/ai-api-keys` as string map.
    pub fn read_ai_keys(&self) -> Result<BTreeMap<String, String>, BaoError> {
        let value = self.send(
            self.http
                .get(self.url(KEYS_PATH))
                .header("X-Vault-Token", &self.token)
                .header("X-Vault-Request", "true"),
        )?;

        let data_map = value
            .pointer("/data/data")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        let mut data = BTreeMap::new();
        for (k, v) in data_map {
            data.insert(k, value_to_string(&v));
        }
        Ok(data)
    }
}

/// Resolve OpenBao address (env → default openbao.boxd.sh).
pub fn resolve_addr() -> String {
    std::env::var("BAO_ADDR")
        .or_else(|_| std::env::var("VAULT_ADDR"))
        .unwrap_or_else(|_| DEFAULT_ADDR.into())
}

/// Prefer env tokens, then `~/.vault-token` / `~/.bao-token`.
pub fn load_stored_token() -> String {
    if let Ok(t) = std::env::var("BAO_TOKEN").or_else(|_| std::env::var("VAULT_TOKEN")) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }

    let home = std::env::var("HOME").ok();
    let candidates: Vec<std::path::PathBuf> = [
        std::env::var("BAO_TOKEN_PATH")
            .ok()
            .map(std::path::PathBuf::from),
        home.as_ref()
            .map(|h| std::path::PathBuf::from(h).join(".vault-token")),
        home.as_ref()
            .map(|h| std::path::PathBuf::from(h).join(".bao-token")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for path in candidates {
        if let Ok(s) = std::fs::read_to_string(&path) {
            let t = s.trim().to_string();
            if !t.is_empty() {
                return t;
            }
        }
    }
    String::new()
}

/// Connect and fetch `ai-api-keys`.
pub fn fetch_ai_keys() -> Result<BTreeMap<String, String>, BaoError> {
    let token = load_stored_token();
    if token.is_empty() {
        return Err(BaoError::NoToken);
    }
    let client = Client::new(&resolve_addr(), &token)?;
    let health = client.health()?;
    if health.sealed {
        return Err(BaoError::Parse("OpenBao is sealed".into()));
    }
    if !health.initialized {
        return Err(BaoError::Parse("OpenBao is not initialized".into()));
    }
    client.read_ai_keys()
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn extract_error_message(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    if let Some(errs) = v.get("errors").and_then(|e| e.as_array()) {
        let joined: Vec<_> = errs
            .iter()
            .filter_map(|e| e.as_str())
            .map(str::to_string)
            .collect();
        if !joined.is_empty() {
            return Some(joined.join("; "));
        }
    }
    v.get("error")
        .and_then(|e| e.as_str())
        .map(str::to_string)
}
