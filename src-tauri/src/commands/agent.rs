//! Tauri commands that expose the agent and its reviewable edit workflow.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::agent;
use crate::model::ModelConfig;
use crate::state::AppState;
use crate::types::{ChatMessage, PendingEdit};

fn model_config(app: &AppHandle) -> Result<ModelConfig, String> {
    let settings = crate::commands::config::resolve(app)?;
    let api_key = settings.api_key.unwrap_or_default();
    if api_key.is_empty() && !is_local_endpoint(&settings.base_url) {
        return Err(
            "No API key configured. Set ORBIT_API_KEY (or DEEPSEEK_API_KEY), or point the Base URL at a local Ollama server (http://localhost:11434/v1) to run without a key.".to_string(),
        );
    }
    Ok(ModelConfig {
        base_url: settings.base_url,
        api_key,
        model: settings.model,
        temperature: settings.temperature,
    })
}

/// Local model servers (Ollama) need no API key.
fn is_local_endpoint(base_url: &str) -> bool {
    base_url.contains("localhost")
        || base_url.contains("127.0.0.1")
        || base_url.contains("0.0.0.0")
        || base_url.contains(":11434")
}

/// Starts an agent run and returns its run id. Progress is streamed over the
/// `orbit://agent` event channel.
#[tauri::command]
pub async fn chat(
    app: AppHandle,
    history: Vec<ChatMessage>,
    prompt: String,
    workspace: String,
) -> Result<String, String> {
    let config = model_config(&app)?;

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let run_id = format!("run-{millis}");

    {
        let state = app.state::<AppState>();
        state
            .active_runs
            .lock()
            .unwrap()
            .insert(run_id.clone(), Arc::new(AtomicBool::new(false)));
    }

    let app_for_task = app.clone();
    let workspace = PathBuf::from(workspace);
    let run_id_for_task = run_id.clone();
    tauri::async_runtime::spawn(async move {
        agent::run_agent(app_for_task, config, run_id_for_task, history, prompt, workspace).await;
    });

    Ok(run_id)
}

/// Requests cancellation of a running agent turn.
#[tauri::command]
pub fn stop_chat(app: AppHandle, run_id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let runs = state.active_runs.lock().unwrap();
    let flag = runs.get(&run_id).ok_or_else(|| "run not found".to_string())?;
    flag.store(true, Ordering::Relaxed);
    Ok(())
}

/// Lists all pending (unreviewed) file edits.
#[tauri::command]
pub fn list_pending_edits(state: State<'_, AppState>) -> Vec<PendingEdit> {
    let edits = state.pending_edits.lock().unwrap();
    let mut list: Vec<PendingEdit> = edits.values().cloned().collect();
    list.sort_by_key(|e| {
        e.id
            .split('-')
            .last()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    });
    list
}

fn apply_one(edit: &PendingEdit) -> Result<(), String> {
    let path = Path::new(&edit.path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
    }
    std::fs::write(path, &edit.after).map_err(|e| format!("write {}: {e}", edit.path))
}

/// Applies a single pending edit to disk.
#[tauri::command]
pub fn apply_pending_edit(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let edit = {
        let edits = state.pending_edits.lock().unwrap();
        edits.get(&id).cloned().ok_or_else(|| "edit not found".to_string())?
    };
    apply_one(&edit)?;
    state.pending_edits.lock().unwrap().remove(&id);
    Ok(())
}

/// Discards a pending edit without applying it.
#[tauri::command]
pub fn discard_pending_edit(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .pending_edits
        .lock()
        .unwrap()
        .remove(&id)
        .map(|_| ())
        .ok_or_else(|| "edit not found".to_string())
}

/// Applies every pending edit and clears the review queue.
#[tauri::command]
pub fn apply_all_pending_edits(state: State<'_, AppState>) -> Result<usize, String> {
    let edits: Vec<PendingEdit> = {
        let guard = state.pending_edits.lock().unwrap();
        guard.values().cloned().collect()
    };
    for edit in &edits {
        apply_one(edit)?;
    }
    state.pending_edits.lock().unwrap().clear();
    Ok(edits.len())
}
