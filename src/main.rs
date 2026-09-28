mod app;
pub mod audio;
mod config;
mod credential;
pub mod display;
pub mod modes;
mod network;
pub mod sd;
pub mod status;

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let app = app::App::new();

    if let Err(error) = app.run() {
        log::error!("Application startup failed: {}", error);
    }
}
