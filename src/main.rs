//! Chat — desktop binary.

use chat::ChatApp;
use vidya::with_app_icon_id;

/// FreeDesktop app id — must match `uk.nandi.chat.desktop` StartupWMClass.
const APP_ID: &str = "uk.nandi.chat";

/// Window / FreeDesktop icon (256² PNG; source SVG in `assets/uk.nandi.chat.svg`).
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/uk.nandi.chat-256.png");

fn main() -> eframe::Result {
    let viewport = with_app_icon_id(
        egui::ViewportBuilder::default()
            .with_inner_size([880.0, 640.0])
            .with_min_inner_size([420.0, 360.0])
            .with_title("Chat"),
        APP_ID,
        APP_ICON_PNG,
    );

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(ChatApp::new(cc)))),
    )
}
