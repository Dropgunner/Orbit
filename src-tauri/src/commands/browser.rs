//! An in-app browser window for Discord Web.
//!
//! The window loads the real `https://discord.com/app` over HTTPS in the
//! platform webview, so the user signs in on Discord's own login page. The app
//! does not inject scripts into the page, does not read its contents or its
//! session, and does not automate it.
//!
//! Security properties:
//! - HTTPS with the platform's certificate validation; no proxy or interception.
//! - Top-level navigation is restricted to Discord's own domains, so a link in a
//!   message cannot steer the signed-in session onto an arbitrary site.
//! - The window is not listed in any capability, so the loaded page gets no
//!   access to the app's Tauri commands.
//! - No initialization script is registered, so no app code runs inside the page.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// The window label, used to focus an existing window instead of opening two.
const WINDOW_LABEL: &str = "discord-web";

/// Discord's web client.
const DISCORD_WEB: &str = "https://discord.com/app";

/// Domains the embedded browser may navigate to. Sub-resources (scripts, images,
/// captcha assets) are not affected by this check.
const ALLOWED_HOSTS: &[&str] = &["discord.com", "discordapp.com", "discord.gg", "hcaptcha.com"];

/// Whether the webview may navigate to `url`.
fn navigation_allowed(url: &tauri::Url) -> bool {
    match url.scheme() {
        "about" | "blob" | "data" => true,
        "https" | "http" => match url.host_str() {
            Some(host) => {
                let host = host.to_ascii_lowercase();
                ALLOWED_HOSTS
                    .iter()
                    .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
            }
            None => false,
        },
        _ => false,
    }
}

/// Open (or focus) the in-app Discord Web browser window.
#[tauri::command]
pub async fn open_discord_web(app: AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(WINDOW_LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }

    let url: tauri::Url = DISCORD_WEB.parse().map_err(|e| format!("bad url: {e}"))?;
    WebviewWindowBuilder::new(&app, WINDOW_LABEL, WebviewUrl::External(url))
        .title("Discord Web")
        .inner_size(1280.0, 860.0)
        .min_inner_size(720.0, 480.0)
        .on_navigation(navigation_allowed)
        .build()
        .map_err(|e| format!("open Discord Web window: {e}"))?;
    Ok(())
}

/// Whether the in-app Discord Web window is currently open.
#[tauri::command]
pub fn discord_web_open(app: AppHandle) -> bool {
    app.get_webview_window(WINDOW_LABEL).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> tauri::Url {
        value.parse().expect("test url")
    }

    #[test]
    fn allows_discord_domains() {
        assert!(navigation_allowed(&url("https://discord.com/app")));
        assert!(navigation_allowed(&url("https://discord.com/channels/@me")));
        assert!(navigation_allowed(&url("https://canary.discord.com/app")));
        assert!(navigation_allowed(&url("https://discord.gg/abc")));
        assert!(navigation_allowed(&url("https://newassets.hcaptcha.com/x")));
        assert!(navigation_allowed(&url("about:blank")));
    }

    #[test]
    fn blocks_other_sites() {
        assert!(!navigation_allowed(&url("https://evil.example/phish")));
        assert!(!navigation_allowed(&url("http://discord.com.evil.example/app")));
        assert!(!navigation_allowed(&url("file:///etc/passwd")));
    }
}
