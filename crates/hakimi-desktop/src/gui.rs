//! Optional Tauri 2 GUI entry (feature = "gui").
//!
//! Expects a running local backend URL; navigates the main window there so
//! Studio WS + REST share origin with the WebUI.

use std::sync::Mutex;

use tauri::Manager;

/// Shared backend base URL for the Tauri process.
pub static BACKEND_URL: Mutex<Option<String>> = Mutex::new(None);

pub fn run_gui(backend_url: String) {
    *BACKEND_URL.lock().unwrap() = Some(backend_url.clone());
    let url = backend_url;

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                // Navigate to local Studio backend (WebUI + /v1/studio).
                let target = url.trim_end_matches('/').to_string() + "/";
                if let Err(e) = window.eval(&format!("window.location.replace({target:?})")) {
                    tracing::warn!(error = %e, "failed to navigate webview; open {target} manually");
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Hakimi Studio desktop");
}
