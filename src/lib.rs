//! Chat — Vidya / egui LLM client (keys from OpenBao).
//!
//! Desktop entry: `src/main.rs`. Android NativeActivity: `android/` package.

mod app;
mod bao;
mod chat;
mod markdown;
mod math;
mod providers;
mod session;

pub use app::ChatApp;
pub use bao::{fetch_ai_keys, set_token_override};
