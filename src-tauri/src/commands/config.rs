//! Application settings: an OpenAI-compatible endpoint, model, temperature,
//! and optional API key. Environment variables override the settings file.

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::types::{Settings, SettingsView};

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    Ok(dir.join("settings.json"))
}

fn read_settings_file(app: &AppHandle) -> Settings {
    let Ok(path) = settings_path(app) else {
        return Settings::default();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Settings::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_settings_file(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

fn env_override(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

/// Merges the persisted settings with environment overrides (env wins).
pub(crate) fn resolve(app: &AppHandle) -> Result<Settings, String> {
    let mut settings = read_settings_file(app);

    if let Some(base_url) = env_override("ORBIT_BASE_URL") {
        settings.base_url = base_url;
    }
    if let Some(model) = env_override("ORBIT_MODEL") {
        settings.model = model;
    }
    if let Some(t) = env_override("ORBIT_TEMPERATURE") {
        if let Ok(v) = t.parse::<f64>() {
            settings.temperature = v;
        }
    }
    if let Some(key) = env_override("ORBIT_API_KEY").or_else(|| env_override("DEEPSEEK_API_KEY")) {
        settings.api_key = Some(key);
    }
    if let Some(key) = env_override("TYPESAFE_API_KEY") {
        settings.typesafe_api_key = Some(key);
    }
    if let Some(base_url) = env_override("TYPESAFE_BASE_URL") {
        settings.typesafe_base_url = base_url;
    }
    if let Some(model) = env_override("TYPESAFE_MODEL") {
        settings.typesafe_model = model;
    }
    if let Some(token) = env_override("DISCORD_TOKEN") {
        settings.discord_token = Some(token);
    }

    Ok(settings)
}

/// Returns the effective settings (file merged with environment).
#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    resolve(&app)
}

/// Persists settings to the settings file.
#[tauri::command]
pub fn set_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    write_settings_file(&app, &settings)
}

/// Alias of get_settings kept for a stable IPC surface.
#[tauri::command]
pub fn resolve_settings(app: AppHandle) -> Result<Settings, String> {
    resolve(&app)
}

/// The effective settings with every secret removed, for the local web bridge.
/// The bridge is reachable by anything holding the per-run token, so it must
/// never be able to read an API key or the Discord token back out.
pub(crate) fn resolve_redacted(app: &AppHandle) -> Result<SettingsView, String> {
    Ok(SettingsView::from(&resolve(app)?))
}

/// Persist settings sent from the bridge, keeping secrets it was never shown.
pub(crate) fn set_settings_preserving_secrets(
    app: &AppHandle,
    incoming: Settings,
) -> Result<(), String> {
    let existing = read_settings_file(app);
    write_settings_file(app, &incoming.preserving_secrets(&existing))
}
