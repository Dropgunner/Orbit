//! The agent loop: stream model turns, execute tool calls, stage file edits
//! for review, and emit progress events to the webview.
//!
//! The same loop backs three modes:
//! - an interactive turn (edits staged for review),
//! - a self-hosting turn (the workspace is the Orbit repository itself), and
//! - one cycle of an autonomous chain (edits applied directly, verification
//!   handled by [`crate::chain`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};

use crate::events::{AgentEvent, AGENT_EVENT};
use crate::model::{stream_chat, ModelConfig};
use crate::selfhost::SelfHost;
use crate::tools;
use crate::types::ChatMessage;

const MAX_ITERATIONS: usize = 25;

/// The marker a chained agent prints when it believes the goal is met.
pub const CHAIN_DONE_MARKER: &str = "ORBIT_CHAIN_DONE";
/// The marker a chained agent prints when it cannot proceed.
pub const CHAIN_BLOCKED_MARKER: &str = "ORBIT_CHAIN_BLOCKED";

/// Everything one agent run needs.
pub struct AgentRequest {
    pub config: ModelConfig,
    pub run_id: String,
    pub history: Vec<ChatMessage>,
    pub prompt: String,
    pub workspace: PathBuf,
    /// Set when the workspace builds Orbit itself.
    pub selfhost: Option<SelfHost>,
    /// When true, file edits are written to disk immediately instead of being
    /// staged for review. Only chain runs set this.
    pub auto_apply: bool,
    /// When true, the agent is told to end its reply with a chain marker.
    pub chained: bool,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    /// The model produced a final answer without tool calls.
    Answered,
    /// The model hit the tool-iteration ceiling.
    IterationLimit,
    /// The run was cancelled by the user.
    Cancelled,
    /// The model reported it could not proceed.
    Blocked(String),
    /// The model reported the goal is met (chain runs only).
    GoalMet,
    /// The transport or a tool failed unrecoverably.
    Failed(String),
}

/// The observable result of one agent run, consumed by the chain runner.
#[derive(Debug, Clone)]
pub struct AgentOutcome {
    pub finish: Finish,
    pub text: String,
    /// Number of file edits the run produced.
    pub edits: usize,
    /// Number of model turns consumed.
    pub turns: usize,
}

impl AgentOutcome {
    /// An outcome for a run that stopped before producing a final model answer.
    fn interrupted(finish: Finish, text: String, edits: usize, turns: usize) -> Self {
        Self {
            finish,
            text,
            edits,
            turns,
        }
    }

    /// Whether the run completed its model turn without being interrupted.
    pub fn completed(&self) -> bool {
        matches!(
            self.finish,
            Finish::Answered | Finish::GoalMet | Finish::IterationLimit
        )
    }

    /// Whether the model claims the goal is met.
    pub fn goal_met(&self) -> bool {
        self.finish == Finish::GoalMet
    }

    /// The reason the run stopped short, if any.
    pub fn blocked_reason(&self) -> Option<&str> {
        match &self.finish {
            Finish::Blocked(reason) => Some(reason.as_str()),
            _ => None,
        }
    }
}

/// Runs a full agent turn to completion, emitting progress on the agent event
/// channel. Callers should invoke this inside `tauri::async_runtime::spawn`.
pub async fn run_agent(app: AppHandle, request: AgentRequest) -> AgentOutcome {
    let run_id = request.run_id.clone();
    let cancel = app
        .state::<crate::state::AppState>()
        .active_runs
        .lock()
        .unwrap()
        .get(&run_id)
        .cloned();

    let outcome = run_inner(&app, request, cancel.as_deref()).await;

    let _ = app
        .state::<crate::state::AppState>()
        .active_runs
        .lock()
        .unwrap()
        .remove(&run_id);

    if let Finish::Failed(error) = &outcome.finish {
        let _ = app.emit(
            AGENT_EVENT,
            AgentEvent::Error {
                run_id,
                error: error.clone(),
            },
        );
    }

    outcome
}

async fn run_inner(
    app: &AppHandle,
    request: AgentRequest,
    cancel: Option<&AtomicBool>,
) -> AgentOutcome {
    let AgentRequest {
        config,
        run_id,
        history,
        prompt,
        workspace,
        selfhost,
        auto_apply,
        chained,
    } = request;
    let run_id = run_id.as_str();
    let workspace = workspace.as_path();

    let tool_defs = tools::tool_definitions();

    let mut messages: Vec<ChatMessage> = Vec::with_capacity(history.len() + 2);
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt(workspace, selfhost.as_ref(), chained),
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

    // Assistant text reaches the UI as it streams, so an interrupted run's
    // `text` stays empty rather than duplicating the streamed deltas.
    let final_text = String::new();
    let mut edits = 0usize;
    let mut turns = 0usize;

    for _ in 0..MAX_ITERATIONS {
        if cancelled(cancel) {
            return AgentOutcome::interrupted(Finish::Cancelled, final_text, edits, turns);
        }

        turns += 1;
        let response = {
            let app = app.clone();
            let run_id = run_id.to_string();
            stream_chat(&config, &messages, &tool_defs, move |delta| {
                let _ = app.emit(
                    AGENT_EVENT,
                    AgentEvent::Delta {
                        run_id: run_id.clone(),
                        content: delta.to_string(),
                    },
                );
            })
            .await
        };

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                return AgentOutcome::interrupted(Finish::Failed(error), final_text, edits, turns)
            }
        };

        if cancelled(cancel) {
            return AgentOutcome::interrupted(Finish::Cancelled, final_text, edits, turns);
        }

        if response.tool_calls.is_empty() {
            return finish_run(
                app,
                run_id,
                response.content,
                edits,
                turns,
                chained,
            );
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

            if matches!(call.function.name.as_str(), "write_file" | "edit_file") {
                edits += 1;
            }

            let state = app.state::<crate::state::AppState>();
            let result = tools::execute_tool(
                app,
                &state,
                run_id,
                &call.function.name,
                &args,
                workspace,
                auto_apply,
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

        if cancelled(cancel) {
            return AgentOutcome::interrupted(Finish::Cancelled, final_text, edits, turns);
        }
    }

    let text = if final_text.is_empty() {
        format!("stopped after {MAX_ITERATIONS} tool iterations without a final answer")
    } else {
        final_text
    };
    AgentOutcome {
        finish: Finish::IterationLimit,
        text,
        edits,
        turns,
    }
}

/// Completes a run whose model turn carried no tool calls, translating any
/// chain marker into a finish reason and emitting a terminal done event.
fn finish_run(
    app: &AppHandle,
    run_id: &str,
    text: String,
    edits: usize,
    turns: usize,
    chained: bool,
) -> AgentOutcome {
    let finish = if !chained {
        Finish::Answered
    } else if let Some(reason) = marker_reason(&text, CHAIN_BLOCKED_MARKER) {
        Finish::Blocked(reason)
    } else if text.contains(CHAIN_DONE_MARKER) {
        Finish::GoalMet
    } else {
        // A chained turn with no marker means the model stopped early; the
        // chain runner verifies and decides whether to continue.
        Finish::Answered
    };

    let _ = app.emit(
        AGENT_EVENT,
        AgentEvent::Done {
            run_id: run_id.to_string(),
            message: text.clone(),
        },
    );

    AgentOutcome {
        finish,
        text,
        edits,
        turns,
    }
}

/// Extracts the free-text reason that follows a marker, if present.
fn marker_reason(text: &str, marker: &str) -> Option<String> {
    let index = text.find(marker)?;
    let rest = text[index + marker.len()..].trim_start_matches([':', '-', ' ']);
    let line = rest.lines().next().unwrap_or("").trim();
    Some(if line.is_empty() {
        "the agent reported it could not proceed".to_string()
    } else {
        line.to_string()
    })
}

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false)
}

fn system_prompt(workspace: &Path, selfhost: Option<&SelfHost>, chained: bool) -> String {
    let mut prompt = String::new();
    prompt.push_str("You are Orbit, an agentic AI coding assistant embedded in a macOS IDE.\n");
    match selfhost {
        Some(host) => prompt.push_str(&host.prompt_section()),
        None => prompt.push_str(&crate::selfhost::default_prompt_section(workspace)),
    }
    prompt.push_str(
        "\nTools: list directories, read files, search the codebase, write or edit files, and run shell commands.\n\
Guidelines:\n\
- Inspect the relevant files before editing them; be precise and minimal.\n\
- Prefer edit_file with a small unique old_string for surgical changes; use write_file to create or fully rewrite a file.\n",
    );
    if chained {
        prompt.push_str(
            "- File edits in this run are applied to disk immediately so you can build and test them. Verify your own work with the project's commands before finishing.\n\
- When the goal is achieved and verification passes, end your reply with the single line ORBIT_CHAIN_DONE.\n\
- If you cannot proceed (missing information, repeated failure, or a decision only the user can make), end your reply with ORBIT_CHAIN_BLOCKED: <one-line reason>.\n",
        );
    } else {
        prompt.push_str(
            "- Every file change is staged for the user's review before it is applied. After proposing changes, summarize them clearly.\n",
        );
    }
    prompt.push_str(
        "- Use run_command to build, test, lint, or inspect git state, and report the results.\n\
- Respond concisely. Show relevant code only when it helps.",
    );
    prompt
}
