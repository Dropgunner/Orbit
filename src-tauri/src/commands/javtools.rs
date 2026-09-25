//! Tauri commands for the seven generic Jev (TypeSafe) toolkit tools. Each
//! resolves the shared client from settings and delegates to `crate::javtools`.

use tauri::AppHandle;

use crate::commands::discord::resolve_typesafe;
use crate::javtools::{self, CompactEntry};

#[tauri::command]
pub async fn classify_cmd(
    app: AppHandle,
    text: String,
    instructions: String,
    labels: Vec<String>,
) -> Result<javtools::ClassifyResult, String> {
    let client = resolve_typesafe(&app)?;
    javtools::classify(&client, &text, &instructions, &labels).await
}

#[tauri::command]
pub async fn score_cmd(
    app: AppHandle,
    text: String,
    instructions: String,
    rubric: Vec<String>,
) -> Result<javtools::ScoreResult, String> {
    let client = resolve_typesafe(&app)?;
    javtools::score_text(&client, &text, &instructions, &rubric).await
}

#[tauri::command]
pub async fn psych_profile_cmd(app: AppHandle, text: String) -> Result<javtools::PsychProfileResult, String> {
    let client = resolve_typesafe(&app)?;
    javtools::psych_profile(&client, &text).await
}

#[tauri::command]
pub async fn noul_cmd(
    app: AppHandle,
    text: String,
    instructions: String,
    yes: Option<String>,
    no: Option<String>,
) -> Result<javtools::NoulResult, String> {
    let client = resolve_typesafe(&app)?;
    javtools::noul_gate(&client, &text, &instructions, yes.as_deref(), no.as_deref()).await
}

#[tauri::command]
pub async fn prune_cmd(
    app: AppHandle,
    text: String,
    goal: Option<String>,
    chunk_lines: Option<usize>,
    keep_threshold: Option<f64>,
) -> Result<javtools::PruneResult, String> {
    let client = resolve_typesafe(&app)?;
    javtools::prune(
        &client,
        &text,
        goal.as_deref(),
        chunk_lines.unwrap_or_else(javtools::default_chunk_lines),
        keep_threshold.unwrap_or_else(javtools::default_keep_threshold),
    )
    .await
}

#[tauri::command]
pub async fn compact_cmd(
    app: AppHandle,
    entries: Vec<CompactEntry>,
    goal: Option<String>,
    preserve_recent: Option<usize>,
    keep_threshold: Option<f64>,
) -> Result<javtools::CompactResult, String> {
    let client = resolve_typesafe(&app)?;
    javtools::compact(
        &client,
        &entries,
        goal.as_deref(),
        preserve_recent.unwrap_or_else(javtools::default_preserve_recent),
        keep_threshold.unwrap_or_else(javtools::default_compact_keep_threshold),
    )
    .await
}

#[tauri::command]
pub async fn generate_cmd(
    app: AppHandle,
    prompt: String,
    candidates: Vec<String>,
    max_tokens: Option<usize>,
    stop: Option<Vec<String>>,
) -> Result<javtools::GenerateResult, String> {
    let client = resolve_typesafe(&app)?;
    javtools::generate(
        &client,
        &prompt,
        &candidates,
        max_tokens.unwrap_or_else(javtools::default_max_tokens),
        &stop.unwrap_or_default(),
    )
    .await
}
