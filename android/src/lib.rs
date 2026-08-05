//! Chat — Android NativeActivity (`android_main`).
//!
//! Desktop sessions use the root crate (`cargo run` / `nix run`).

use chat::ChatApp;
use vidya::with_app_icon_id;

const APP_TITLE: &str = "Chat";
const APP_ID: &str = "uk.nandi.chat";

/// Window / launcher icon (256² PNG; source SVG in `assets/uk.nandi.chat.svg`).
const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/uk.nandi.chat-256.png");

#[cfg(not(target_os = "android"))]
pub fn run_desktop() -> eframe::Result {
    let viewport = with_app_icon_id(
        eframe::egui::ViewportBuilder::default()
            .with_inner_size([420.0, 720.0])
            .with_title(APP_TITLE),
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

#[cfg(target_os = "android")]
pub fn run_android(android_app: winit::platform::android::activity::AndroidApp) -> eframe::Result {
    let viewport = with_app_icon_id(
        eframe::egui::ViewportBuilder::default().with_title(APP_TITLE),
        APP_ID,
        APP_ICON_PNG,
    );
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    options.android_app = Some(android_app);
    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(ChatApp::new(cc)))),
    )
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android_app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    log::info!("chat android_main start");
    match run_android(android_app) {
        Ok(()) => log::info!("chat run_android returned Ok"),
        Err(e) => log::error!("chat run_android error: {e}"),
    }
}
