//! Chat — Vidya / egui LLM client (keys from OpenBao).

mod app;
mod bao;
mod chat;
mod markdown;
mod providers;

use app::ChatApp;

/// FreeDesktop app id — must match `uk.nandi.chat.desktop` StartupWMClass.
const APP_ID: &str = "uk.nandi.chat";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([880.0, 640.0])
            .with_min_inner_size([420.0, 360.0])
            .with_title("Chat")
            .with_app_id(APP_ID),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(ChatApp::new(cc)))),
    )
}
