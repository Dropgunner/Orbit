//! Serde data types shared across the Rust backend and mirrored by the
//! TypeScript IPC contract in `src/ipc/contract.ts`.

use serde::{Deserialize, Serialize};

/// User-facing model settings. The API key is optional here because it may be
/// supplied through the environment instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    #[serde(default)]
    pub api_key: Option<String>,
    /// TypeSafe (Jev) API key for profiling/guardrails.
    #[serde(default)]
    pub typesafe_api_key: Option<String>,
    /// TypeSafe API root.
    #[serde(default = "default_typesafe_base_url")]
    pub typesafe_base_url: String,
    /// TypeSafe model name.
    #[serde(default = "default_typesafe_model")]
    pub typesafe_model: String,
    /// Discord user token for live browsing and DiscordChatExporter exports.
    #[serde(default)]
    pub discord_token: Option<String>,
    /// Optional explicit path to the DeepSeek Harness CLI (`dsh`). When empty,
    /// the executable is looked up on PATH.
    #[serde(default)]
    pub dsh_path: Option<String>,
}

fn default_typesafe_base_url() -> String {
    "https://api.typesafe.ai".to_string()
}

fn default_typesafe_model() -> String {
    "jev-latest".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
            temperature: 0.2,
            api_key: None,
            typesafe_api_key: None,
            typesafe_base_url: default_typesafe_base_url(),
            typesafe_model: default_typesafe_model(),
            discord_token: None,
            dsh_path: None,
        }
    }
}

/// Settings with every secret removed, for the local web bridge. Each `*_set`
/// flag says whether a secret exists without disclosing it, so the browser can
/// still show which providers are configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    pub typesafe_base_url: String,
    pub typesafe_model: String,
    pub api_key_set: bool,
    pub typesafe_api_key_set: bool,
    pub discord_token_set: bool,
    /// Not secret: the browser UI may edit the harness path.
    pub dsh_path: Option<String>,
}

impl From<&Settings> for SettingsView {
    fn from(settings: &Settings) -> Self {
        let is_set = |value: &Option<String>| {
            value.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false)
        };
        Self {
            base_url: settings.base_url.clone(),
            model: settings.model.clone(),
            temperature: settings.temperature,
            typesafe_base_url: settings.typesafe_base_url.clone(),
            typesafe_model: settings.typesafe_model.clone(),
            api_key_set: is_set(&settings.api_key),
            typesafe_api_key_set: is_set(&settings.typesafe_api_key),
            discord_token_set: is_set(&settings.discord_token),
            dsh_path: settings.dsh_path.clone(),
        }
    }
}

impl Settings {
    /// Keep the stored secret when an incoming update omits it. The browser is
    /// only ever shown redacted settings, so a save from there must not erase a
    /// key the user cannot see.
    pub fn preserving_secrets(mut self, existing: &Settings) -> Self {
        let keep = |incoming: Option<String>, stored: &Option<String>| match incoming {
            Some(value) if !value.trim().is_empty() => Some(value),
            _ => stored.clone(),
        };
        self.api_key = keep(self.api_key.clone(), &existing.api_key);
        self.typesafe_api_key = keep(self.typesafe_api_key.clone(), &existing.typesafe_api_key);
        self.discord_token = keep(self.discord_token.clone(), &existing.discord_token);
        self
    }
}

/// One entry in a directory listing.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_ms: Option<u64>,
}

/// A single match from a text search.
#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
}

/// An OpenAI-compatible chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A tool call attached to an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// A proposed file change, staged for the user's review before it is written
/// to disk. `before == None` means the file does not exist yet (creation).
#[derive(Debug, Clone, Serialize)]
pub struct PendingEdit {
    pub id: String,
    pub path: String,
    pub description: String,
    pub before: Option<String>,
    pub after: String,
    pub created: bool,
}

/// Repository status returned by `git status --porcelain=v1 --branch`.
#[derive(Debug, Clone, Serialize)]
pub struct GitStatus {
    pub branch: String,
    pub entries: Vec<GitStatusEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitStatusEntry {
    pub path: String,
    pub index: String,
    pub worktree: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitCommit {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Settings {
        Settings {
            api_key: Some("deepseek-secret".to_string()),
            typesafe_api_key: Some("typesafe-secret".to_string()),
            discord_token: Some("discord-secret".to_string()),
            ..Settings::default()
        }
    }

    #[test]
    fn redacted_view_reports_presence_without_disclosing_secrets() {
        let view = SettingsView::from(&stored());
        assert!(view.api_key_set && view.typesafe_api_key_set && view.discord_token_set);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("secret"), "{json}");
        assert!(!json.contains("api_key\""), "{json}");
    }

    #[test]
    fn redacted_view_marks_absent_secrets() {
        let view = SettingsView::from(&Settings::default());
        assert!(!view.api_key_set && !view.typesafe_api_key_set && !view.discord_token_set);
    }

    #[test]
    fn saving_redacted_settings_keeps_stored_secrets() {
        // What the browser can send back: it never saw the secrets.
        let incoming = Settings { api_key: None, typesafe_api_key: None, discord_token: None, ..Settings::default() };
        let merged = incoming.preserving_secrets(&stored());
        assert_eq!(merged.api_key.as_deref(), Some("deepseek-secret"));
        assert_eq!(merged.typesafe_api_key.as_deref(), Some("typesafe-secret"));
        assert_eq!(merged.discord_token.as_deref(), Some("discord-secret"));
    }

    #[test]
    fn saving_a_new_secret_replaces_the_stored_one() {
        let incoming = Settings { api_key: Some("fresh".to_string()), ..Settings::default() };
        let merged = incoming.preserving_secrets(&stored());
        assert_eq!(merged.api_key.as_deref(), Some("fresh"));
        // Untouched secrets still survive alongside the replacement.
        assert_eq!(merged.discord_token.as_deref(), Some("discord-secret"));
    }

    #[test]
    fn blank_secret_is_treated_as_no_change() {
        let incoming = Settings { api_key: Some("   ".to_string()), ..Settings::default() };
        let merged = incoming.preserving_secrets(&stored());
        assert_eq!(merged.api_key.as_deref(), Some("deepseek-secret"));
    }

    #[test]
    fn settings_saved_before_the_harness_field_still_load() {
        // A settings.json written by an older build has no `dsh_path`.
        let legacy = r#"{
            "base_url": "https://api.deepseek.com",
            "model": "deepseek-chat",
            "temperature": 0.2
        }"#;
        let parsed: Settings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.model, "deepseek-chat");
        assert!(parsed.dsh_path.is_none());
        assert_eq!(parsed.typesafe_model, "jev-latest");
    }

    #[test]
    fn harness_path_survives_redaction() {
        let settings = Settings {
            dsh_path: Some("/opt/homebrew/bin/dsh".to_string()),
            ..Settings::default()
        };
        let view = SettingsView::from(&settings);
        assert_eq!(view.dsh_path.as_deref(), Some("/opt/homebrew/bin/dsh"));
        // It is not a secret, so it must not be routed through a `*_set` flag.
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("dsh_path"));
    }
}
