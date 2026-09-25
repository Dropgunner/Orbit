//! Tauri commands for autonomous chains: start one, watch it, stop it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::chain::{self, ChainRequest, ChainRun, DEFAULT_MAX_CYCLES, MAX_CYCLES_CAP};
use crate::commands::agent::{detect_selfhost, model_config};
use crate::state::AppState;

/// Starts a chained autonomous run and returns its chain id. Progress streams
/// over `orbit://chain`, and each cycle's model output over `orbit://agent`.
///
/// The workspace must be self-hosting for verification to have anything to run;
/// a chain in an ordinary project still works, but only the agent's own
/// judgement decides when it is done.
#[tauri::command]
pub async fn start_chain(
    app: AppHandle,
    goal: String,
    workspace: String,
    max_cycles: Option<usize>,
) -> Result<String, String> {
    if goal.trim().is_empty() {
        return Err("a chain needs a goal".to_string());
    }
    let config = model_config(&app)?;
    let max_cycles = max_cycles.unwrap_or(DEFAULT_MAX_CYCLES).clamp(1, MAX_CYCLES_CAP);
    let selfhost = detect_selfhost(&workspace);

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let id = format!("chain-{millis}");
    let cancel = Arc::new(AtomicBool::new(false));

    {
        let state = app.state::<AppState>();
        state.chains.lock().unwrap().insert(
            id.clone(),
            ChainRun {
                id: id.clone(),
                goal: goal.clone(),
                cycle: 0,
                max_cycles,
                status: chain::ChainStatus::Running.as_str().to_string(),
                log: vec![format!(
                    "chain started (up to {max_cycles} cycle(s)){}",
                    if selfhost.is_some() {
                        ", self-hosting workspace"
                    } else {
                        ""
                    }
                )],
                cancel: cancel.clone(),
            },
        );
    }

    let app_for_task = app.clone();
    let id_for_task = id.clone();
    tauri::async_runtime::spawn(async move {
        chain::run_chain(
            app_for_task,
            id_for_task,
            config,
            ChainRequest {
                goal,
                workspace: std::path::PathBuf::from(workspace),
                selfhost,
                max_cycles,
            },
        )
        .await;
    });

    Ok(id)
}

/// Requests cancellation of a running chain.
#[tauri::command]
pub fn stop_chain(app: AppHandle, chain_id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let chains = state.chains.lock().unwrap();
    let run = chains
        .get(&chain_id)
        .ok_or_else(|| "chain not found".to_string())?;
    run.cancel.store(true, Ordering::Relaxed);
    Ok(())
}

/// Returns a chain's current state, or every chain when no id is given.
#[tauri::command]
pub fn get_chain(state: State<'_, AppState>, chain_id: Option<String>) -> Vec<ChainRun> {
    let chains = state.chains.lock().unwrap();
    match chain_id {
        Some(id) => chains.get(&id).cloned().into_iter().collect(),
        None => {
            let mut all: Vec<ChainRun> = chains.values().cloned().collect();
            all.sort_by(|a, b| a.id.cmp(&b.id));
            all
        }
    }
}
