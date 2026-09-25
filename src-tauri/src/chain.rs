//! Chained autonomous cycles: the agent proposes and applies changes, Orbit
//! verifies them with the project's own commands, and a failed verification is
//! fed back as the next cycle's prompt until it passes or the budget runs out.
//!
//! The decision logic ([`next_action`]) is pure so the loop's stopping rules are
//! unit-tested without invoking a model or a shell.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::agent::{AgentOutcome, AgentRequest, Finish};
use crate::events::{ChainEvent, CHAIN_EVENT};
use crate::model::ModelConfig;
use crate::selfhost::SelfHost;
use crate::tools::{self, CommandCapture};

/// Default ceiling on autonomous cycles for one chain.
pub const DEFAULT_MAX_CYCLES: usize = 4;
/// Hard ceiling, so a caller cannot request an unbounded autonomous loop.
pub const MAX_CYCLES_CAP: usize = 12;
/// How long one verification command may run.
pub const VERIFY_TIMEOUT_SECS: u64 = 600;

/// What the chain should do after a cycle finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// Goal met and verification passed.
    Complete,
    /// Run another cycle with this prompt.
    Retry(String),
    /// Stop without success and report this reason.
    GiveUp(String),
}

/// How the chain as a whole ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStatus {
    /// Goal met and verification passed.
    Passed,
    /// No budget or no progress.
    Failed,
    /// The user stopped it.
    Cancelled,
    /// The agent reported it could not proceed.
    Blocked,
    /// Still running.
    Running,
}

impl ChainStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ChainStatus::Passed => "passed",
            ChainStatus::Failed => "failed",
            ChainStatus::Cancelled => "cancelled",
            ChainStatus::Blocked => "blocked",
            ChainStatus::Running => "running",
        }
    }
}

/// Why a verification did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyFailure {
    /// No verification command is known for this workspace.
    NoCommand,
    /// The command ran and exited non-zero.
    Command {
        label: String,
        code: Option<i32>,
        output: String,
    },
    /// The command could not be started or timed out.
    NotRun(String),
}

impl VerifyFailure {
    /// A human-readable reason for the chain log.
    pub fn reason(&self) -> String {
        match self {
            VerifyFailure::NoCommand => "no verification command is known for this workspace".to_string(),
            VerifyFailure::Command { label, code, .. } => format!(
                "{label} failed (exit {})",
                code.map_or("killed".to_string(), |c| c.to_string())
            ),
            VerifyFailure::NotRun(error) => format!("verification could not run: {error}"),
        }
    }

    /// The prompt fragment handed to the next cycle.
    pub fn feedback(&self) -> String {
        match self {
            VerifyFailure::NoCommand => {
                "No verification command is available for this workspace, so state explicitly what you changed and how you checked it."
                    .to_string()
            }
            VerifyFailure::Command { label, code, output } => format!(
                "The verification step `{label}` FAILED (exit {}). Fix the cause of this failure:\n\n{}",
                code.map_or("killed".to_string(), |c| c.to_string()),
                output
            ),
            VerifyFailure::NotRun(error) => {
                format!("The verification step could not be run: {error}")
            }
        }
    }
}

/// Decides the next step of a chain.
///
/// Precedence: a user cancellation always wins; a blocked report stops the
/// chain; the goal is only complete when the model says so *and* verification
/// has passed.
pub fn next_action(
    outcome: &AgentOutcome,
    verification: Option<&Result<CommandCapture, String>>,
    cycle: usize,
    max_cycles: usize,
) -> NextAction {
    if outcome.finish == Finish::Cancelled {
        return NextAction::GiveUp("stopped by the user".to_string());
    }
    if let Some(reason) = outcome.blocked_reason() {
        return NextAction::GiveUp(format!("the agent reported it was blocked: {reason}"));
    }
    if let Finish::Failed(error) = &outcome.finish {
        return NextAction::GiveUp(format!("the agent run failed: {error}"));
    }

    let verified = match verification {
        Some(Ok(capture)) if capture.succeeded() => Some(()),
        _ => None,
    };
    if outcome.goal_met() && verified.is_some() {
        return NextAction::Complete;
    }

    if cycle >= max_cycles {
        return NextAction::GiveUp(format!(
            "stopped after {max_cycles} cycle(s) without a passing verification"
        ));
    }

    let mut prompt = String::new();
    match (outcome.goal_met(), verification) {
        // The model believed it was done but the build disagreed: this is the
        // case the chain exists for. (A passing verification already returned.)
        (true, _) => {
            prompt.push_str(
                "You reported that the goal was met, but the verification step below did not pass. Do not restate the previous answer: find the actual cause and fix it.\n\n",
            );
            prompt.push_str(&failure_feedback(verification));
        }
        _ => {
            prompt.push_str("Continue working toward the goal. Verification has not passed yet.\n\n");
            prompt.push_str(&failure_feedback(verification));
        }
    }
    NextAction::Retry(prompt)
}

fn failure_feedback(verification: Option<&Result<CommandCapture, String>>) -> String {
    match verification {
        Some(Ok(capture)) => VerifyFailure::Command {
            label: "the project's check".to_string(),
            code: capture.code,
            output: capture.output.clone(),
        }
        .feedback(),
        Some(Err(error)) => VerifyFailure::NotRun(error.clone()).feedback(),
        None => VerifyFailure::NoCommand.feedback(),
    }
}

/// A chain as reported to the UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChainRun {
    /// Identifier for this chain.
    pub id: String,
    /// What the user asked for.
    pub goal: String,
    /// Cycles started so far.
    pub cycle: usize,
    /// Cycle ceiling.
    pub max_cycles: usize,
    /// `running` | `passed` | `failed` | `cancelled` | `blocked`.
    pub status: String,
    /// One human-readable line per step.
    pub log: Vec<String>,
    /// Cancellation flag shared with the agent loop.
    #[serde(skip)]
    pub cancel: Arc<AtomicBool>,
}

/// Runs a chain to completion, emitting progress on the chain event channel.
pub async fn run_chain(app: AppHandle, id: String, config: ModelConfig, request: ChainRequest) {
    let ChainRequest {
        goal,
        workspace,
        selfhost,
        max_cycles,
    } = request;

    // The verification commands are written relative to the repository root
    // (the directory that owns both `orbit/` and `src/`).
    let repo_root = repo_root(&workspace);
    let cancel = {
        let state = app.state::<crate::state::AppState>();
        let chains = state.chains.lock().unwrap();
        chains.get(&id).map(|run| run.cancel.clone())
    };
    let cancel = match cancel {
        Some(flag) => flag,
        None => return,
    };

    let mut prompt = initial_prompt(&goal, selfhost.as_ref());
    let mut status = ChainStatus::Failed;
    let mut final_reason = String::new();

    for cycle in 1..=max_cycles {
        if cancel.load(Ordering::Relaxed) {
            status = ChainStatus::Cancelled;
            final_reason = "stopped by the user".to_string();
            break;
        }

        log_line(&app, &id, format!("cycle {cycle}/{max_cycles} started"));
        {
            let state = app.state::<crate::state::AppState>();
            let mut chains = state.chains.lock().unwrap();
            if let Some(run) = chains.get_mut(&id) {
                run.cycle = cycle;
            }
        }
        let _ = app.emit(
            CHAIN_EVENT,
            ChainEvent::CycleStart {
                chain_id: id.clone(),
                cycle,
                max_cycles,
            },
        );

        let outcome = crate::agent::run_agent(
            app.clone(),
            AgentRequest {
                config: config.clone(),
                run_id: format!("{id}-c{cycle}"),
                history: Vec::new(),
                prompt: prompt.clone(),
                workspace: workspace.clone(),
                selfhost: selfhost.clone(),
                auto_apply: true,
                chained: true,
            },
        )
        .await;

        if cancel.load(Ordering::Relaxed) || outcome.finish == Finish::Cancelled {
            status = ChainStatus::Cancelled;
            final_reason = "stopped by the user".to_string();
            log_line(&app, &id, final_reason.clone());
            break;
        }

        log_line(
            &app,
            &id,
            format!(
                "cycle {cycle} finished: {} tool turn(s), {} edit(s)",
                outcome.turns, outcome.edits
            ),
        );

        // Verify with the project's own commands; skip when the agent is already
        // blocked or the run died, since there is nothing to verify.
        let verification = if outcome.completed() && selfhost.is_some() {
            Some(verify(&app, &id, &repo_root, selfhost.as_ref().unwrap()).await)
        } else {
            None
        };

        if let Some(result) = &verification {
            let label = selfhost
                .as_ref()
                .and_then(|host| host.verify_commands.first())
                .map(|command| command.label.clone())
                .unwrap_or_else(|| "verification".to_string());
            let (ok, detail) = match result {
                Ok(capture) if capture.succeeded() => (true, format!("{label} passed")),
                Ok(capture) => (
                    false,
                    VerifyFailure::Command {
                        label: label.clone(),
                        code: capture.code,
                        output: String::new(),
                    }
                    .reason(),
                ),
                Err(error) => (false, VerifyFailure::NotRun(error.clone()).reason()),
            };
            let _ = app.emit(
                CHAIN_EVENT,
                ChainEvent::Verify {
                    chain_id: id.clone(),
                    cycle,
                    ok,
                    detail: detail.clone(),
                },
            );
            log_line(&app, &id, format!("verification {detail}"));
        }

        match next_action(&outcome, verification.as_ref(), cycle, max_cycles) {
            NextAction::Complete => {
                status = ChainStatus::Passed;
                final_reason = format!("verification passed after {cycle} cycle(s)");
                break;
            }
            NextAction::GiveUp(reason) => {
                status = if outcome.blocked_reason().is_some() {
                    ChainStatus::Blocked
                } else {
                    ChainStatus::Failed
                };
                final_reason = reason;
                break;
            }
            NextAction::Retry(next_prompt) => {
                let _ = app.emit(
                    CHAIN_EVENT,
                    ChainEvent::Cycle {
                        chain_id: id.clone(),
                        cycle,
                        message: outcome.text.clone(),
                        edits: outcome.edits,
                    },
                );
                prompt = next_prompt;
            }
        }
    }

    log_line(&app, &id, final_reason.clone());
    {
        let state = app.state::<crate::state::AppState>();
        let mut chains = state.chains.lock().unwrap();
        if let Some(run) = chains.get_mut(&id) {
            run.status = status.as_str().to_string();
        }
    }
    let _ = app.emit(
        CHAIN_EVENT,
        ChainEvent::Done {
            chain_id: id,
            status: status.as_str().to_string(),
            reason: final_reason,
        },
    );
}

/// Inputs for one chain.
pub struct ChainRequest {
    pub goal: String,
    pub workspace: PathBuf,
    pub selfhost: Option<SelfHost>,
    pub max_cycles: usize,
}

/// The repository root that owns the Orbit checkout.
fn repo_root(workspace: &std::path::Path) -> PathBuf {
    let nested = workspace.join("orbit").join("src-tauri").join("Cargo.toml");
    if nested.is_file() {
        workspace.to_path_buf()
    } else {
        workspace
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| workspace.to_path_buf())
    }
}

/// The first prompt of a chain.
fn initial_prompt(goal: &str, selfhost: Option<&SelfHost>) -> String {
    let mut prompt = String::new();
    prompt.push_str("Work autonomously on this goal until it is complete and verified:\n\n");
    prompt.push_str(goal.trim());
    prompt.push_str("\n\n");
    if let Some(host) = selfhost {
        prompt.push_str("You are changing Orbit itself. The verification step runs on its own between your turns:\n");
        for command in &host.verify_commands {
            prompt.push_str(&format!("- {}: `{}`\n", command.label, command.command));
        }
    }
    prompt.push_str(
        "\nWork in small, compiling steps. You may edit files and run commands yourself. \
When you believe the goal is met, end your reply with ORBIT_CHAIN_DONE; \
if you are blocked, end with ORBIT_CHAIN_BLOCKED: <reason>.",
    );
    prompt
}

fn log_line(app: &AppHandle, id: &str, line: String) {
    let state = app.state::<crate::state::AppState>();
    let mut chains = state.chains.lock().unwrap();
    if let Some(run) = chains.get_mut(id) {
        run.log.push(line.clone());
    }
    let _ = app.emit(
        CHAIN_EVENT,
        ChainEvent::Log {
            chain_id: id.to_string(),
            line,
        },
    );
}

/// Runs the first verification command that the workspace declares.
async fn verify(
    app: &AppHandle,
    id: &str,
    repo_root: &std::path::Path,
    selfhost: &SelfHost,
) -> Result<CommandCapture, String> {
    let Some(command) = selfhost.verify_commands.first() else {
        return Err("no verification command is declared for this workspace".to_string());
    };
    log_line(app, id, format!("running `{}`", command.command));
    tools::run_command_capture(
        repo_root,
        &command.command,
        std::time::Duration::from_secs(VERIFY_TIMEOUT_SECS),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(finish: Finish) -> AgentOutcome {
        AgentOutcome {
            finish,
            text: "did work".to_string(),
            edits: 1,
            turns: 2,
        }
    }

    fn passing() -> Result<CommandCapture, String> {
        Ok(CommandCapture {
            output: "ok".to_string(),
            code: Some(0),
        })
    }

    fn failing() -> Result<CommandCapture, String> {
        Ok(CommandCapture {
            output: "error[E0425]: cannot find value".to_string(),
            code: Some(101),
        })
    }

    #[test]
    fn completes_only_when_goal_met_and_verified() {
        let action = next_action(&outcome(Finish::GoalMet), Some(&passing()), 1, 4);
        assert_eq!(action, NextAction::Complete);
    }

    #[test]
    fn goal_without_verification_does_not_complete() {
        let action = next_action(&outcome(Finish::GoalMet), Some(&failing()), 1, 4);
        match action {
            NextAction::Retry(prompt) => {
                assert!(prompt.contains("verification step"));
                assert!(prompt.contains("E0425"));
            }
            other => panic!("expected retry, got {other:?}"),
        }
    }

    #[test]
    fn goal_with_no_verification_command_retries_then_gives_up() {
        assert!(matches!(
            next_action(&outcome(Finish::GoalMet), None, 1, 2),
            NextAction::Retry(_)
        ));
        assert!(matches!(
            next_action(&outcome(Finish::GoalMet), None, 2, 2),
            NextAction::GiveUp(_)
        ));
    }

    #[test]
    fn verification_error_is_fed_back_and_retried() {
        let failed = Err("verification command timed out after 600s".to_string());
        match next_action(&outcome(Finish::GoalMet), Some(&failed), 1, 4) {
            NextAction::Retry(prompt) => assert!(prompt.contains("timed out")),
            other => panic!("expected retry, got {other:?}"),
        }
    }

    #[test]
    fn cancellation_wins_over_everything() {
        let action = next_action(&outcome(Finish::Cancelled), Some(&passing()), 1, 4);
        assert!(matches!(action, NextAction::GiveUp(ref r) if r.contains("user")));
    }

    #[test]
    fn blocked_report_stops_the_chain_immediately() {
        let blocked = outcome(Finish::Blocked("needs an API key".to_string()));
        match next_action(&blocked, Some(&failing()), 1, 4) {
            NextAction::GiveUp(reason) => {
                assert!(reason.contains("blocked"));
                assert!(reason.contains("needs an API key"));
            }
            other => panic!("expected give-up, got {other:?}"),
        }
    }

    #[test]
    fn transport_failure_gives_up_rather_than_looping() {
        let failed = outcome(Finish::Failed("connection refused".to_string()));
        match next_action(&failed, None, 1, 4) {
            NextAction::GiveUp(reason) => assert!(reason.contains("connection refused")),
            other => panic!("expected give-up, got {other:?}"),
        }
    }

    #[test]
    fn unanswered_cycle_continues_without_claiming_success() {
        let action = next_action(&outcome(Finish::Answered), Some(&failing()), 1, 4);
        match action {
            NextAction::Retry(prompt) => assert!(prompt.contains("Continue working")),
            other => panic!("expected retry, got {other:?}"),
        }
    }

    #[test]
    fn initial_prompt_lists_verification_commands() {
        let host = SelfHost {
            project: "Orbit".to_string(),
            has_desktop_app: true,
            has_profiler_cli: false,
            has_plugin: false,
            verify_commands: vec![crate::selfhost::VerifyCommand {
                label: "rust check".to_string(),
                command: "cargo check".to_string(),
            }],
        };
        let prompt = initial_prompt("add a button", Some(&host));
        assert!(prompt.contains("add a button"));
        assert!(prompt.contains("cargo check"));
        assert!(prompt.contains("ORBIT_CHAIN_DONE"));
    }

    #[test]
    fn verify_failure_messages_describe_the_cause() {
        let failure = VerifyFailure::Command {
            label: "rust tests".to_string(),
            code: Some(101),
            output: "assertion failed".to_string(),
        };
        assert!(failure.reason().contains("101"));
        assert!(failure.feedback().contains("assertion failed"));
    }
}
