//! Tauri commands for the local web UI.

use tauri::AppHandle;

/// Start the local web server (idempotent), open it in the default browser, and
/// return its token-bearing URL.
#[tauri::command]
pub async fn start_web_ui(app: AppHandle) -> Result<String, String> {
    let info = crate::web::start(&app).await?;
    let url = info.url();
    open_browser(&url);
    Ok(url)
}

/// Stop the local web server, if running.
#[tauri::command]
pub fn stop_web_ui(app: AppHandle) {
    crate::web::stop(&app);
}

/// The current web UI URL, or `null` when the server is not running.
#[tauri::command]
pub fn web_ui_url(app: AppHandle) -> Option<String> {
    crate::web::current(&app).map(|info| info.url())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
