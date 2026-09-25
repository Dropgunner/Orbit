//! Tauri commands for the Discord analysis mode: import, browse, run the Jev
//! (TypeSafe) profile/bot/guard pipelines, flag entities and patterns, and
//! stream a DeepSeek discussion of an analysis report.

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager};

use crate::assets::assets;
use crate::discord::{import, DiscordDataset, DiscordMessage, DiscordUser, EntityFlag};
use crate::events::{DiscordEvent, DISCORD_EVENT};
use crate::model::{stream_chat, ModelConfig};
use crate::patterns::{
    build_pattern_report, normalize_pattern, validate_pattern, BotPattern, MessageInput,
    PatternInput, PatternMatchReport,
};
use crate::profiler::{
    run_bot, run_guard, run_profile, screen_message, BotReport, GuardReport, ProfileReport,
    TypeSafeClient,
};
use crate::state::AppState;
use crate::types::ChatMessage;

/// Summary of the currently imported dataset (everything the UI needs to list
/// guild, channels, and users without pulling every message over IPC).
#[derive(serde::Serialize)]
pub struct DatasetSummary {
    guild: crate::discord::DiscordGuild,
    channels: Vec<crate::discord::DiscordChannel>,
    users: Vec<DiscordUser>,
    message_count: usize,
}

#[derive(serde::Serialize)]
pub struct GuardPolicies {
    policies: Vec<String>,
    default: String,
}

pub(crate) fn to_summary(dataset: &DiscordDataset) -> DatasetSummary {
    DatasetSummary {
        guild: dataset.guild.clone(),
        channels: dataset.channels.clone(),
        users: dataset.users.clone(),
        message_count: dataset.messages.len(),
    }
}

fn with_dataset<T>(
    app: &AppHandle,
    f: impl FnOnce(&DiscordDataset) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<AppState>();
    let guard = state.discord_dataset.lock().unwrap();
    let dataset = guard.as_ref().ok_or_else(|| "No Discord data imported yet.".to_string())?;
    f(dataset)
}

/// Resolve the TypeSafe (Jev) client from settings + environment.
pub(crate) fn resolve_typesafe(app: &AppHandle) -> Result<TypeSafeClient, String> {
    let settings = crate::commands::config::resolve(app)?;
    let api_key = settings.typesafe_api_key.unwrap_or_default();
    TypeSafeClient::new(settings.typesafe_base_url, api_key, settings.typesafe_model)
}

/// Resolve the DeepSeek (OpenAI-compatible) client config for discussions.
fn resolve_model_config(app: &AppHandle) -> Result<ModelConfig, String> {
    let settings = crate::commands::config::resolve(app)?;
    let api_key = settings.api_key.unwrap_or_default();
    if api_key.is_empty() && !is_local_endpoint(&settings.base_url) {
        return Err(
            "No DeepSeek API key configured. Set it in Settings, or point the Base URL at a local Ollama server to discuss without a key.".to_string(),
        );
    }
    Ok(ModelConfig {
        base_url: settings.base_url,
        api_key,
        model: settings.model,
        temperature: settings.temperature,
    })
}

fn is_local_endpoint(base_url: &str) -> bool {
    base_url.contains("localhost")
        || base_url.contains("127.0.0.1")
        || base_url.contains("0.0.0.0")
        || base_url.contains(":11434")
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn import_discord(app: AppHandle, path: String) -> Result<DatasetSummary, String> {
    let content = fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    let name_hint = PathBuf::from(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Discord export".to_string());
    let dataset = import(&content, &name_hint)?;
    let summary = to_summary(&dataset);
    *app.state::<AppState>().discord_dataset.lock().unwrap() = Some(dataset);
    Ok(summary)
}

#[tauri::command]
pub fn import_discord_text(app: AppHandle, name: String, content: String) -> Result<DatasetSummary, String> {
    let dataset = import(&content, &name)?;
    let summary = to_summary(&dataset);
    *app.state::<AppState>().discord_dataset.lock().unwrap() = Some(dataset);
    Ok(summary)
}

#[tauri::command]
pub fn discord_summary(app: AppHandle) -> Result<DatasetSummary, String> {
    with_dataset(&app, |dataset| Ok(to_summary(dataset)))
}

#[tauri::command]
pub fn discord_messages(
    app: AppHandle,
    user_id: Option<String>,
    channel_id: Option<String>,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<DiscordMessage>, String> {
    let limit = limit.unwrap_or(500);
    with_dataset(&app, |dataset| {
        let mut out: Vec<DiscordMessage> = dataset
            .messages
            .iter()
            .filter(|m| user_id.as_ref().map(|u| &m.author_id == u).unwrap_or(true))
            .filter(|m| channel_id.as_ref().map(|c| m.channel_id.as_ref() == Some(c)).unwrap_or(true))
            .filter(|m| {
                query
                    .as_ref()
                    .map(|q| m.content.to_lowercase().contains(&q.to_lowercase()))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        out.truncate(limit);
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

fn user_contents(
    app: &AppHandle,
    user_id: &str,
    limit: usize,
) -> Result<(Vec<String>, Vec<Option<String>>, Option<String>), String> {
    with_dataset(app, |dataset| {
        let user = dataset
            .user(user_id)
            .ok_or_else(|| format!("user \"{user_id}\" not found"))?;
        let author = Some(user.name.clone());
        let mut contents = Vec::new();
        let mut timestamps = Vec::new();
        for message in dataset.messages_for(user_id) {
            if limit > 0 && contents.len() >= limit {
                break;
            }
            contents.push(message.content.clone());
            timestamps.push(message.timestamp.clone());
        }
        Ok((contents, timestamps, author))
    })
}

#[tauri::command]
pub async fn analyze_profile(
    app: AppHandle,
    user_id: String,
    limit: Option<usize>,
    dry_run: Option<bool>,
) -> Result<ProfileReport, String> {
    let (contents, timestamps, author) = user_contents(&app, &user_id, limit.unwrap_or(200))?;
    if contents.is_empty() {
        return Err("This user has no messages.".to_string());
    }
    let dry_run = dry_run.unwrap_or(false);
    let client = if dry_run {
        TypeSafeClient::new("https://api.typesafe.ai".to_string(), "dry-run".to_string(), "jev-latest".to_string())
            .unwrap_or_else(|_| unreachable!())
    } else {
        resolve_typesafe(&app)?
    };
    run_profile(&client, &user_id, author.as_deref(), &contents, &timestamps, &[], 4, dry_run).await
}

#[tauri::command]
pub async fn analyze_bot(
    app: AppHandle,
    user_id: String,
    limit: Option<usize>,
    dry_run: Option<bool>,
) -> Result<BotReport, String> {
    let (contents, timestamps, _author) = user_contents(&app, &user_id, limit.unwrap_or(200))?;
    if contents.is_empty() {
        return Err("This user has no messages.".to_string());
    }
    let dry_run = dry_run.unwrap_or(false);
    let client = if dry_run {
        TypeSafeClient::new("https://api.typesafe.ai".to_string(), "dry-run".to_string(), "jev-latest".to_string())
            .unwrap_or_else(|_| unreachable!())
    } else {
        resolve_typesafe(&app)?
    };
    run_bot(&client, &user_id, &contents, &timestamps, &[], 4, dry_run).await
}

#[tauri::command]
pub async fn analyze_guard(
    app: AppHandle,
    user_id: String,
    policy: Option<String>,
    limit: Option<usize>,
    dry_run: Option<bool>,
) -> Result<GuardReport, String> {
    let (contents, timestamps, author) = user_contents(&app, &user_id, limit.unwrap_or(200))?;
    if contents.is_empty() {
        return Err("This user has no messages.".to_string());
    }
    let dry_run = dry_run.unwrap_or(false);
    let policy = policy.unwrap_or_else(|| assets().default_guard_policy.clone());
    let client = if dry_run {
        TypeSafeClient::new("https://api.typesafe.ai".to_string(), "dry-run".to_string(), "jev-latest".to_string())
            .unwrap_or_else(|_| unreachable!())
    } else {
        resolve_typesafe(&app)?
    };
    run_guard(&client, &user_id, author.as_deref(), &contents, &timestamps, &[], 4, &policy, dry_run).await
}

#[tauri::command]
pub async fn screen_message_cmd(
    app: AppHandle,
    content: String,
    side: Option<String>,
    policy: Option<String>,
    dry_run: Option<bool>,
) -> Result<GuardReport, String> {
    let side = side.unwrap_or_else(|| "input".to_string());
    let policy = policy.unwrap_or_else(|| assets().default_guard_policy.clone());
    let dry_run = dry_run.unwrap_or(false);
    let client = if dry_run {
        TypeSafeClient::new("https://api.typesafe.ai".to_string(), "dry-run".to_string(), "jev-latest".to_string())
            .unwrap_or_else(|_| unreachable!())
    } else {
        resolve_typesafe(&app)?
    };
    screen_message(&client, &side, &content, &policy, dry_run).await
}

#[tauri::command]
pub fn guard_policies() -> GuardPolicies {
    let mut policies: Vec<String> = assets().guard_policies.keys().cloned().collect();
    policies.sort();
    GuardPolicies {
        policies,
        default: assets().default_guard_policy.clone(),
    }
}

// ---------------------------------------------------------------------------
// Pattern store
// ---------------------------------------------------------------------------

fn patterns_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| format!("config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    Ok(dir.join("patterns.json"))
}

fn flags_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| format!("config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    Ok(dir.join("flags.json"))
}

fn load_patterns(app: &AppHandle) -> Vec<BotPattern> {
    let Ok(path) = patterns_path(app) else { return Vec::new() };
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    let Ok(parsed) = serde_json::from_str::<Vec<BotPattern>>(&text) else { return Vec::new() };
    parsed.into_iter().filter(|p| validate_pattern(p).is_ok()).collect()
}

fn save_patterns(app: &AppHandle, patterns: &[BotPattern]) -> Result<(), String> {
    for pattern in patterns {
        validate_pattern(pattern)?;
    }
    let path = patterns_path(app)?;
    let text = serde_json::to_string_pretty(patterns).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

fn load_flags(app: &AppHandle) -> Vec<EntityFlag> {
    let Ok(path) = flags_path(app) else { return Vec::new() };
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    serde_json::from_str::<Vec<EntityFlag>>(&text).unwrap_or_default()
}

fn save_flags(app: &AppHandle, flags: &[EntityFlag]) -> Result<(), String> {
    let path = flags_path(app)?;
    let text = serde_json::to_string_pretty(flags).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_patterns(app: AppHandle) -> Vec<BotPattern> {
    load_patterns(&app)
}

#[tauri::command]
pub fn flag_pattern(app: AppHandle, input: PatternInput) -> Result<Vec<BotPattern>, String> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let candidate = normalize_pattern(input, &now)?;
    let mut store = load_patterns(&app);
    if let Some(existing) = store.iter_mut().find(|p| p.id == candidate.id) {
        let created = existing.created_at.clone();
        let mut updated = candidate;
        updated.created_at = created;
        *existing = updated;
    } else {
        store.push(candidate);
    }
    save_patterns(&app, &store)?;
    Ok(store)
}

#[tauri::command]
pub fn remove_pattern(app: AppHandle, id: String) -> Result<Vec<BotPattern>, String> {
    let store = load_patterns(&app);
    let next: Vec<BotPattern> = store.into_iter().filter(|p| p.id != id).collect();
    save_patterns(&app, &next)?;
    Ok(next)
}

#[tauri::command]
pub fn match_patterns(
    app: AppHandle,
    user_id: Option<String>,
    channel_id: Option<String>,
) -> Result<PatternMatchReport, String> {
    let store = load_patterns(&app);
    if store.is_empty() {
        return Err("No flagged patterns yet. Flag one first.".to_string());
    }
    with_dataset(&app, |dataset| {
        let messages: Vec<MessageInput> = dataset
            .messages
            .iter()
            .filter(|m| user_id.as_ref().map(|u| &m.author_id == u).unwrap_or(true))
            .filter(|m| channel_id.as_ref().map(|c| m.channel_id.as_ref() == Some(c)).unwrap_or(true))
            .enumerate()
            .map(|(i, m)| MessageInput {
                index: i + 1,
                content: m.content.clone(),
                timestamp: m.timestamp.clone(),
            })
            .collect();
        let source = dataset.guild.name.clone();
        build_pattern_report(&source, &store, &messages)
    })
}

// ---------------------------------------------------------------------------
// Manual flags
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_flags(app: AppHandle) -> Vec<EntityFlag> {
    load_flags(&app)
}

#[tauri::command]
pub fn set_flag(
    app: AppHandle,
    entity_type: String,
    entity_id: String,
    label: String,
    action: String,
    note: String,
) -> Result<Vec<EntityFlag>, String> {
    if entity_type != "user" && entity_type != "message" {
        return Err("entity_type must be \"user\" or \"message\"".to_string());
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut flags = load_flags(&app);
    flags.retain(|f| !(f.entity_type == entity_type && f.entity_id == entity_id));
    flags.push(EntityFlag {
        entity_type,
        entity_id,
        label,
        action,
        note,
        created_at: now,
    });
    save_flags(&app, &flags)?;
    Ok(flags)
}

#[tauri::command]
pub fn clear_flag(app: AppHandle, entity_type: String, entity_id: String) -> Result<Vec<EntityFlag>, String> {
    let mut flags = load_flags(&app);
    flags.retain(|f| !(f.entity_type == entity_type && f.entity_id == entity_id));
    save_flags(&app, &flags)?;
    Ok(flags)
}

// ---------------------------------------------------------------------------
// DeepSeek discussion
// ---------------------------------------------------------------------------

/// The system prompt plus the user's question, shared by both discussion paths.
fn discussion_messages(prompt: String, context: String) -> Vec<ChatMessage> {
    let system = format!(
        "You are a careful, skeptical analyst helping a human interpret a TypeSafe psychological/bot-detection/guardrails report about Discord users. Do not treat the report as a verdict about a person. Distinguish what the evidence supports from speculation, flag inverted-risk signals without judgment, and note self-harm signals with care (route them to support, never blame).\n\nAnalysis context:\n{context}"
    );
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: prompt,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ]
}

#[tauri::command]
pub async fn discuss(app: AppHandle, prompt: String, context: String) -> Result<String, String> {
    let config = resolve_model_config(&app)?;
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let run_id = format!("discuss-{millis}");

    let app_for_task = app.clone();
    let run_id_for_task = run_id.clone();
    tauri::async_runtime::spawn(async move {
        let messages = discussion_messages(prompt, context);
        let on_delta = |delta: &str| {
            let _ = app_for_task.emit(
                DISCORD_EVENT,
                DiscordEvent::Delta {
                    run_id: run_id_for_task.clone(),
                    content: delta.to_string(),
                },
            );
        };

        match stream_chat(&config, &messages, &[], on_delta).await {
            Ok(response) => {
                let _ = app_for_task.emit(
                    DISCORD_EVENT,
                    DiscordEvent::Done {
                        run_id: run_id_for_task.clone(),
                        message: response.content,
                    },
                );
            }
            Err(error) => {
                let _ = app_for_task.emit(
                    DISCORD_EVENT,
                    DiscordEvent::Error {
                        run_id: run_id_for_task.clone(),
                        error,
                    },
                );
            }
        }
    });

    Ok(run_id)
}

/// One-shot discussion returning the full reply. Used by the local web UI, which
/// has no Tauri event channel for streaming deltas.
#[tauri::command]
pub async fn discuss_once(app: AppHandle, prompt: String, context: String) -> Result<String, String> {
    let config = resolve_model_config(&app)?;
    let messages = discussion_messages(prompt, context);
    let response = stream_chat(&config, &messages, &[], |_| {}).await?;
    Ok(response.content)
}
