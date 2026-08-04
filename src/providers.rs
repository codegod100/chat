//! OpenAI-compatible provider registry built from OpenBao key map.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
    /// When true, fetch `/models` for the picker.
    pub fetch_models: bool,
}

#[derive(Debug, Clone)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

/// Known providers keyed by OpenBao secret field name.
const SPECS: &[ProviderSpec] = &[
    ProviderSpec {
        key_name: "OPENROUTER_API_KEY",
        id: "openrouter",
        label: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "openrouter/free",
        fetch_models: true,
        preferred: true,
    },
    ProviderSpec {
        key_name: "DEEPSEEK_API_KEY",
        id: "deepseek",
        label: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
        fetch_models: true,
        preferred: false,
    },
    ProviderSpec {
        key_name: "OPENCODE_API_KEY",
        id: "opencode",
        label: "OpenCode Zen",
        base_url: "https://opencode.ai/zen/v1",
        default_model: "deepseek-v4-flash-free",
        fetch_models: false,
        preferred: false,
    },
    ProviderSpec {
        key_name: "OLLAMA_API_KEY",
        id: "ollama",
        label: "Ollama",
        base_url: "https://ollama.com/v1",
        default_model: "llama3.2",
        fetch_models: true,
        preferred: false,
    },
];

struct ProviderSpec {
    key_name: &'static str,
    id: &'static str,
    label: &'static str,
    base_url: &'static str,
    default_model: &'static str,
    fetch_models: bool,
    preferred: bool,
}

/// Build providers present in the OpenBao key map (preferred first).
pub fn from_keys(keys: &BTreeMap<String, String>) -> Vec<Provider> {
    let mut out = Vec::new();
    let mut preferred = None;

    for spec in SPECS {
        let Some(api_key) = keys.get(spec.key_name) else {
            continue;
        };
        let api_key = api_key.trim();
        if api_key.is_empty() {
            continue;
        }
        let p = Provider {
            id: spec.id,
            label: spec.label,
            base_url: spec.base_url.to_string(),
            api_key: api_key.to_string(),
            default_model: spec.default_model.to_string(),
            fetch_models: spec.fetch_models,
        };
        if spec.preferred && preferred.is_none() {
            preferred = Some(p);
        } else {
            out.push(p);
        }
    }

    if let Some(p) = preferred {
        out.insert(0, p);
    }
    out
}

/// Built-in free tier when no OpenBao token (OpenCode Zen, no API key required).
pub fn free_defaults() -> Vec<Provider> {
    vec![Provider {
        id: "opencode",
        label: "OpenCode Zen (free)",
        base_url: "https://opencode.ai/zen/v1".into(),
        api_key: String::new(),
        default_model: "deepseek-v4-flash-free".into(),
        fetch_models: true,
    }]
}

fn is_free_model_id(id: &str) -> bool {
    id == "big-pickle" || id.contains("-free")
}

fn with_auth(
    req: reqwest::blocking::RequestBuilder,
    provider: &Provider,
) -> reqwest::blocking::RequestBuilder {
    if provider.api_key.is_empty() {
        return req;
    }
    req.header("Authorization", format!("Bearer {}", provider.api_key))
}

pub fn list_models(provider: &Provider) -> Result<Vec<ModelChoice>, String> {
    if !provider.fetch_models {
        return Ok(vec![ModelChoice {
            id: provider.default_model.clone(),
            label: provider.default_model.clone(),
        }]);
    }

    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/models", provider.base_url.trim_end_matches('/'));
    let mut req = with_auth(http.get(&url), provider);

    if provider.id == "openrouter" {
        req = req
            .header("HTTP-Referer", "https://nandi.uk/chat")
            .header("X-Title", "Chat");
    }

    let response = req.send().map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "models HTTP {}: {}",
            status.as_u16(),
            body.chars().take(200).collect::<String>()
        ));
    }

    let parsed: ModelsResponse =
        serde_json::from_str(&body).map_err(|e| format!("models parse: {e}"))?;

    let mut models: Vec<ModelChoice> = parsed
        .data
        .into_iter()
        .filter(|m| provider.api_key.is_empty() || is_free_model_id(&m.id))
        .map(|m| ModelChoice {
            id: m.id.clone(),
            label: m.id,
        })
        .collect();

    if provider.id == "openrouter" {
        // Prefer free models near the top for the picker.
        models.sort_by(|a, b| {
            let af = a.id.contains(":free") || a.id == "openrouter/free";
            let bf = b.id.contains(":free") || b.id == "openrouter/free";
            match (af, bf) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.id.cmp(&b.id),
            }
        });
    } else {
        models.sort_by(|a, b| a.id.cmp(&b.id));
    }

    if models.is_empty() {
        models.push(ModelChoice {
            id: provider.default_model.clone(),
            label: provider.default_model.clone(),
        });
    }

    Ok(models)
}

/// Pick a sensible default model id from the listed models.
pub fn pick_default_model(provider: &Provider, models: &[ModelChoice]) -> String {
    if models.iter().any(|m| m.id == provider.default_model) {
        return provider.default_model.clone();
    }
    if provider.id == "openrouter" {
        if let Some(m) = models.iter().find(|m| m.id.contains(":free")) {
            return m.id.clone();
        }
    }
    if provider.id == "opencode" && provider.api_key.is_empty() {
        if let Some(m) = models
            .iter()
            .find(|m| m.id == provider.default_model)
        {
            return m.id.clone();
        }
        if let Some(m) = models.iter().find(|m| is_free_model_id(&m.id)) {
            return m.id.clone();
        }
    }
    models
        .first()
        .map(|m| m.id.clone())
        .unwrap_or_else(|| provider.default_model.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_defaults_use_opencode_zen() {
        let providers = free_defaults();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "opencode");
        assert!(providers[0].api_key.is_empty());
        assert_eq!(providers[0].default_model, "deepseek-v4-flash-free");
    }

    #[test]
    fn free_model_ids() {
        assert!(is_free_model_id("deepseek-v4-flash-free"));
        assert!(is_free_model_id("big-pickle"));
        assert!(!is_free_model_id("deepseek-v4-flash"));
    }
}
