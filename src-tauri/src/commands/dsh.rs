//! Tauri commands for driving the DeepSeek Harness CLI from Orbit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::dsh::{self, DshStatus};
use crate::state::AppState;

/// Reports whether the Harness CLI can be driven from this machine.
#[tauri::command]
pub fn dsh_status(app: AppHandle) -> DshStatus {
    dsh::status(&app)
}

/// Starts one one-shot Harness task and returns its run id. Output streams on
/// `orbit://dsh`.
#[tauri::command]
pub async fn dsh_run(
    app: AppHandle,
    prompt: String,
    workspace: String,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("a harness task needs a prompt".to_string());
    }
    if !dsh::status(&app).available {
        return Err(
            "The DeepSeek Harness CLI (`dsh`) was not found. Install it, or set its path in Settings."
                .to_string(),
        );
    }

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let run_id = format!("dsh-{millis}");
    let cancel = Arc::new(AtomicBool::new(false));

    {
        let state = app.state::<AppState>();
        state
            .dsh_runs
            .lock()
            .unwrap()
            .insert(run_id.clone(), cancel.clone());
    }

    let app_for_task = app.clone();
    let run_id_for_task = run_id.clone();
    tauri::async_runtime::spawn(async move {
        dsh::run(
            app_for_task.clone(),
            run_id_for_task.clone(),
            prompt,
            std::path::PathBuf::from(workspace),
            cancel,
        )
        .await;
        app_for_task
            .state::<AppState>()
            .dsh_runs
            .lock()
            .unwrap()
            .remove(&run_id_for_task);
    });

    Ok(run_id)
}

/// Requests cancellation of a running Harness task.
#[tauri::command]
pub fn stop_dsh(app: AppHandle, run_id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let runs = state.dsh_runs.lock().unwrap();
    let flag = runs
        .get(&run_id)
        .ok_or_else(|| "harness run not found".to_string())?;
    flag.store(true, Ordering::Relaxed);
    Ok(())
}
