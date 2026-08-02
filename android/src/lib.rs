//! Chat — Android NativeActivity (`android_main`).
//!
//! Desktop sessions use the root crate (`cargo run` / `nix run`).

use chat::ChatApp;

const APP_TITLE: &str = "Chat";
const APP_ID: &str = "uk.nandi.chat";

#[cfg(not(target_os = "android"))]
pub fn run_desktop() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([420.0, 720.0])
            .with_title(APP_TITLE)
            .with_app_id(APP_ID),
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
    let mut options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_title(APP_TITLE),
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
