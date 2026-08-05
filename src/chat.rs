//! Chat messages and streaming OpenAI-compatible completions.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::providers::Provider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    /// Visible assistant reply tokens.
    Delta(String),
    /// Model-internal reasoning is in flight (not shown in the chat bubble).
    ReasoningDelta,
    Done,
    Error(String),
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ApiMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

pub struct StreamHandle {
    pub rx: Receiver<ChatEvent>,
    cancel: Arc<AtomicBool>,
}

impl StreamHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Spawn a background streaming completion. Returns a handle for UI polling.
pub fn start_stream(
    provider: Provider,
    model: String,
    history: Vec<Message>,
) -> StreamHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = Arc::clone(&cancel);

    thread::spawn(move || {
        if let Err(e) = run_stream(&provider, &model, &history, &tx, &cancel_worker) {
            let _ = tx.send(ChatEvent::Error(e));
        }
    });

    StreamHandle { rx, cancel }
}

fn run_stream(
    provider: &Provider,
    model: &str,
    history: &[Message],
    tx: &Sender<ChatEvent>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let messages: Vec<ApiMessage<'_>> = history
        .iter()
        .map(|m| ApiMessage {
            role: match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            },
            content: &m.content,
        })
        .collect();

    let body = ChatRequest {
        model,
        messages,
        stream: true,
    };

    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );

    let mut req = http
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body);
    if !provider.api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", provider.api_key));
    }

    if provider.id == "openrouter" {
        req = req
            .header("HTTP-Referer", "https://nandi.uk/chat")
            .header("X-Title", "Chat");
    }

    let response = req.send().map_err(|e| e.to_string())?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().unwrap_or_default();
        return Err(format!(
            "HTTP {}: {}",
            status.as_u16(),
            err_body.chars().take(400).collect::<String>()
        ));
    }

    let reader = BufReader::new(response);
    parse_sse(reader, tx, cancel)?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    let _ = tx.send(ChatEvent::Done);
    Ok(())
}

fn parse_sse<R: Read>(
    reader: BufReader<R>,
    tx: &Sender<ChatEvent>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    for line in reader.lines() {
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        let line = line.map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with("data:") {
            continue;
        }
        let data = line[5..].trim();
        if data == "[DONE]" {
            break;
        }

        let value: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Visible reply text — never mix in reasoning_content.
        let content = value
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
            .or_else(|| {
                value
                    .pointer("/choices/0/message/content")
                    .and_then(|v| v.as_str())
            });

        if let Some(text) = content {
            if !text.is_empty() {
                if tx.send(ChatEvent::Delta(text.to_string())).is_err() {
                    return Ok(());
                }
            }
        }

        let reasoning = value
            .pointer("/choices/0/delta/reasoning_content")
            .and_then(|v| v.as_str())
            .or_else(|| {
                value
                    .pointer("/choices/0/message/reasoning_content")
                    .and_then(|v| v.as_str())
            });

        if let Some(text) = reasoning {
            if !text.is_empty() {
                if tx.send(ChatEvent::ReasoningDelta).is_err() {
                    return Ok(());
                }
            }
        }

        // Surface provider error objects in the stream.
        if let Some(err) = value.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("provider error");
            return Err(msg.to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bao;
    use crate::providers;

    #[test]
    fn stream_one_reply_smoke() {
        if bao::load_stored_token().is_empty() {
            eprintln!("skip: no OpenBao token");
            return;
        }
        let keys = bao::fetch_ai_keys().expect("keys");
        let providers = providers::from_keys(&keys);
        assert!(!providers.is_empty(), "no providers from keys");
        let provider = providers[0].clone();
        let model = provider.default_model.clone();

        let handle = start_stream(
            provider,
            model.clone(),
            vec![Message {
                role: Role::User,
                content: "Reply with exactly: ok".into(),
            }],
        );

        let mut out = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        loop {
            if std::time::Instant::now() > deadline {
                handle.cancel();
                panic!("timeout waiting for stream; so far: {out:?}");
            }
            match handle.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(ChatEvent::Delta(s)) => out.push_str(&s),
                Ok(ChatEvent::ReasoningDelta) => {}
                Ok(ChatEvent::Done) => break,
                Ok(ChatEvent::Error(e)) => panic!("stream error: {e}"),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        eprintln!("model={model} reply={out:?}");
        assert!(!out.trim().is_empty(), "empty assistant reply");
    }
}
