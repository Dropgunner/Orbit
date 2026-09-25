//! Tauri commands for live Discord browsing (native REST client, user token)
//! and DiscordChatExporter CLI exports.

use tauri::{AppHandle, Manager};

use crate::commands::discord::{to_summary, DatasetSummary};
use crate::discord::{import, to_dce_json, DiscordUser};
use crate::discord_api::{build_dataset, ApiMember, DiscordClient};
use crate::state::AppState;

#[derive(serde::Serialize)]
pub struct DiscordAccount {
    pub id: String,
    pub username: String,
    pub global_name: Option<String>,
}

#[derive(serde::Serialize)]
pub struct DiscordGuildInfo {
    pub id: String,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct DiscordChannelInfo {
    pub id: String,
    pub name: String,
}

/// A guild's member roster, or why it could not be read. Discord refuses member
/// enumeration with 403 for some tokens; that refusal must not block the rest of
/// the workspace, so it is reported as a state rather than an error.
#[derive(serde::Serialize)]
pub struct MemberRoster {
    pub users: Vec<DiscordUser>,
    pub unavailable: bool,
    /// Why the roster is unavailable; `None` when it loaded.
    pub reason: Option<String>,
}

#[derive(serde::Serialize)]
pub struct DceExportResult {
    pub path: String,
    pub loaded: bool,
    pub dataset: Option<DatasetSummary>,
}

fn resolve_discord_token(app: &AppHandle) -> Result<String, String> {
    let settings = crate::commands::config::resolve(app)?;
    // Trim: a token pasted with a stray space is otherwise sent verbatim and
    // rejected by Discord as invalid.
    let token = settings.discord_token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Err(
            "No Discord token. Set it in Settings, or set the DISCORD_TOKEN environment variable.".to_string(),
        );
    }
    Ok(token)
}

fn client(app: &AppHandle) -> Result<DiscordClient, String> {
    DiscordClient::new(resolve_discord_token(app)?)
}

/// Validate the token and return the authenticated account.
#[tauri::command]
pub async fn discord_whoami(app: AppHandle) -> Result<DiscordAccount, String> {
    let me = client(&app)?.current_user().await?;
    Ok(DiscordAccount {
        id: me.id,
        username: me.username,
        global_name: me.global_name,
    })
}

#[tauri::command]
pub async fn discord_guilds(app: AppHandle) -> Result<Vec<DiscordGuildInfo>, String> {
    let guilds = client(&app)?.list_guilds().await?;
    Ok(guilds
        .into_iter()
        .map(|g| DiscordGuildInfo { id: g.id, name: g.name })
        .collect())
}

#[tauri::command]
pub async fn discord_channels(app: AppHandle, guild_id: String) -> Result<Vec<DiscordChannelInfo>, String> {
    let channels = client(&app)?.list_channels(&guild_id).await?;
    Ok(channels
        .into_iter()
        .map(|c| DiscordChannelInfo { id: c.id, name: c.name })
        .collect())
}

/// Guild members. A refused member list (403) comes back as an unavailable
/// roster: Discord restricts member enumeration for some tokens, and the channel
/// list and message fetch stay usable without it.
#[tauri::command]
pub async fn discord_members(
    app: AppHandle,
    guild_id: String,
    limit: Option<usize>,
) -> Result<MemberRoster, String> {
    let client = client(&app)?;
    match client.list_members(&guild_id, limit.unwrap_or(1000)).await {
        Ok(members) => Ok(MemberRoster {
            users: roster_users(members),
            unavailable: false,
            reason: None,
        }),
        Err(error) if error.is_forbidden() => Ok(MemberRoster {
            users: Vec::new(),
            unavailable: true,
            reason: Some(error.to_string()),
        }),
        Err(error) => Err(error.to_string()),
    }
}

/// Guild members as displayable users, sorted by name.
fn roster_users(members: Vec<ApiMember>) -> Vec<DiscordUser> {
    let mut users: Vec<DiscordUser> = members
        .into_iter()
        .filter_map(|m| {
            let api_user = m.user?;
            Some(DiscordUser {
                id: api_user.id.clone(),
                name: api_user.display_name(),
                discriminator: nonempty(api_user.discriminator),
                nickname: m.nick.filter(|n| !n.trim().is_empty()),
                is_bot: api_user.bot.unwrap_or(false),
                message_count: 0,
            })
        })
        .collect();
    users.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    users
}

/// Fetch a channel's messages (and member nicknames) into the analysis dataset.
#[tauri::command]
pub async fn discord_fetch_channel(
    app: AppHandle,
    guild_id: Option<String>,
    guild_name: String,
    channel_id: String,
    channel_name: String,
    limit: Option<usize>,
) -> Result<DatasetSummary, String> {
    let client = client(&app)?;
    let messages = client.fetch_messages(&channel_id, limit.unwrap_or(1000)).await?;
    if messages.is_empty() {
        return Err("No messages returned for this channel.".to_string());
    }
    // Member list is best-effort: it only enriches nicknames.
    let members = client.list_members(&guild_id.clone().unwrap_or_default(), 1000).await.unwrap_or_default();
    let dataset = build_dataset(
        guild_id,
        &guild_name,
        Some(channel_id),
        &channel_name,
        messages,
        members,
    );
    let summary = to_summary(&dataset);
    *app.state::<AppState>().discord_dataset.lock().unwrap() = Some(dataset);
    Ok(summary)
}

#[tauri::command]
pub fn discord_dce_status(app: AppHandle) -> crate::dce::DceStatus {
    crate::dce::status(&app)
}

/// Write the current dataset to `path` as DiscordChatExporter-compatible JSON.
#[tauri::command]
pub fn discord_export_json(app: AppHandle, path: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let guard = state.discord_dataset.lock().unwrap();
    let dataset = guard
        .as_ref()
        .ok_or_else(|| "No Discord data to export yet.".to_string())?;
    let value = to_dce_json(dataset);
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("write {path}: {e}"))?;
    Ok(path)
}

/// The current dataset as DiscordChatExporter-compatible JSON, for clients that
/// download it themselves (the local web UI).
#[tauri::command]
pub fn discord_export_json_text(app: AppHandle) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let guard = state.discord_dataset.lock().unwrap();
    let dataset = guard
        .as_ref()
        .ok_or_else(|| "No Discord data to export yet.".to_string())?;
    Ok(to_dce_json(dataset))
}

/// Export a channel with DiscordChatExporter and (for JSON) load it into the app.
#[tauri::command]
pub async fn discord_dce_export(
    app: AppHandle,
    channel_id: String,
    format: Option<String>,
    load_into_app: Option<bool>,
) -> Result<DceExportResult, String> {
    let token = resolve_discord_token(&app)?;
    let format = format.unwrap_or_else(|| "Json".to_string());
    let load = load_into_app.unwrap_or(true);

    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?
        .join("exports");
    let out_file = crate::dce::export_channel(&app, &token, &channel_id, &format, &dir).await?;

    let mut dataset = None;
    if load && format == "Json" {
        let content = std::fs::read_to_string(&out_file).map_err(|e| format!("read export: {e}"))?;
        match import(&content, &channel_id) {
            Ok(parsed) => {
                let summary = to_summary(&parsed);
                *app.state::<AppState>().discord_dataset.lock().unwrap() = Some(parsed);
                dataset = Some(summary);
            }
            Err(e) => return Err(format!("DiscordChatExporter wrote JSON but it could not be imported: {e}")),
        }
    }

    Ok(DceExportResult {
        path: out_file.to_string_lossy().to_string(),
        loaded: dataset.is_some(),
        dataset,
    })
}

fn nonempty(value: String) -> Option<String> {
    let t = value.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}
