//! The agent loop: stream model turns, execute tool calls, stage file edits
//! for review, and emit progress events to the webview.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};

use crate::events::{AgentEvent, AGENT_EVENT};
use crate::model::{stream_chat, ModelConfig};
use crate::tools;
use crate::types::ChatMessage;

const MAX_ITERATIONS: usize = 25;

/// Runs a full agent turn to completion, emitting progress on the agent event
/// channel. Callers should invoke this inside `tauri::async_runtime::spawn`.
pub async fn run_agent(
    app: AppHandle,
    config: ModelConfig,
    run_id: String,
    history: Vec<ChatMessage>,
    prompt: String,
    workspace: PathBuf,
) {
    let cancel = app
        .state::<crate::state::AppState>()
        .active_runs
        .lock()
        .unwrap()
        .get(&run_id)
        .cloned();

    let result = run_inner(
        &app,
        &config,
        &run_id,
        history,
        prompt,
        &workspace,
        cancel.as_deref(),
    )
    .await;

    let _ = app
        .state::<crate::state::AppState>()
        .active_runs
        .lock()
        .unwrap()
        .remove(&run_id);

    if let Err(error) = result {
        let _ = app.emit(
            AGENT_EVENT,
            AgentEvent::Error {
                run_id,
                error,
            },
        );
    }
}

async fn run_inner(
    app: &AppHandle,
    config: &ModelConfig,
    run_id: &str,
    history: Vec<ChatMessage>,
    prompt: String,
    workspace: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    let tool_defs = tools::tool_definitions();

    let mut messages: Vec<ChatMessage> = Vec::with_capacity(history.len() + 2);
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt(workspace),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });
    messages.extend(history);
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: prompt,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    let mut final_text = String::new();
    let mut finished = false;

    for _ in 0..MAX_ITERATIONS {
        if cancelled(cancel) {
            return Err("cancelled".to_string());
        }

        let response = {
            let app = app.clone();
            let run_id = run_id.to_string();
            stream_chat(config, &messages, &tool_defs, move |delta| {
                let _ = app.emit(
                    AGENT_EVENT,
                    AgentEvent::Delta {
                        run_id: run_id.clone(),
                        content: delta.to_string(),
                    },
                );
            })
            .await?
        };

        if cancelled(cancel) {
            return Err("cancelled".to_string());
        }

        if response.tool_calls.is_empty() {
            final_text = response.content;
            finished = true;
            break;
        }

        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.content,
            tool_calls: Some(response.tool_calls.clone()),
            tool_call_id: None,
            name: None,
        });

        for call in &response.tool_calls {
            let _ = app.emit(
                AGENT_EVENT,
                AgentEvent::ToolCall {
                    run_id: run_id.to_string(),
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    arguments: call.function.arguments.clone(),
                },
            );

            let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                .unwrap_or(serde_json::Value::Null);

            let state = app.state::<crate::state::AppState>();
            let result = tools::execute_tool(
                app,
                &state,
                run_id,
                &call.function.name,
                &args,
                workspace,
            )
            .await
            .unwrap_or_else(|e| format!("tool error: {e}"));

            let _ = app.emit(
                AGENT_EVENT,
                AgentEvent::ToolResult {
                    run_id: run_id.to_string(),
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    result: result.clone(),
                },
            );

            messages.push(ChatMessage {
                role: "tool".to_string(),
                content: result,
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
                name: Some(call.function.name.clone()),
            });
        }
    }

    if !finished {
        return Err(format!(
            "stopped after {MAX_ITERATIONS} tool iterations without a final answer"
        ));
    }

    let _ = app.emit(
        AGENT_EVENT,
        AgentEvent::Done {
            run_id: run_id.to_string(),
            message: final_text,
        },
    );
    Ok(())
}

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false)
}

fn system_prompt(workspace: &Path) -> String {
    format!(
        "You are Orbit, an agentic AI coding assistant embedded in a macOS IDE.\n\
You are working in the project at: {}\n\
\n\
You have tools to list directories, read files, search the codebase, propose file edits, and run shell commands.\n\
Guidelines:\n\
- Inspect the relevant files before editing them; be precise and minimal.\n\
- Prefer edit_file with a small unique old_string for surgical changes; use write_file to create or fully rewrite a file.\n\
- Every file change is staged for the user's review before it is applied. After proposing changes, summarize them clearly.\n\
- Use run_command to build, test, lint, or inspect git state, and report the results.\n\
- Respond concisely. Show relevant code only when it helps.",
        workspace.display()
    )
}
