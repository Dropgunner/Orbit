//! Payloads for events streamed from the backend to the webview.

use serde::Serialize;

use crate::types::PendingEdit;

/// Events emitted on the `orbit://agent` channel while a chat run is active.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A text token from the assistant's current reply.
    Delta {
        run_id: String,
        content: String,
    },
    /// The assistant invoked a tool.
    ToolCall {
        run_id: String,
        id: String,
        name: String,
        arguments: String,
    },
    /// A tool finished and returned this result.
    ToolResult {
        run_id: String,
        id: String,
        name: String,
        result: String,
    },
    /// A file change was proposed and is awaiting review.
    PendingEdit {
        run_id: String,
        edit: PendingEdit,
    },
    /// The run completed; `message` is the full final assistant text.
    Done {
        run_id: String,
        message: String,
    },
    /// The run failed or was cancelled.
    Error {
        run_id: String,
        error: String,
    },
}

/// Events emitted on the `orbit://terminal` channel.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalEvent {
    Data { id: String, data: String },
    Exit { id: String, code: Option<i32> },
}

/// Events emitted on the `orbit://discord` channel while a DeepSeek discussion
/// of an analysis report is streaming.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscordEvent {
    /// A text token from the assistant.
    Delta { run_id: String, content: String },
    /// The discussion completed; `message` is the full final text.
    Done { run_id: String, message: String },
    /// The discussion failed.
    Error { run_id: String, error: String },
}

/// Events emitted on the `orbit://chain` channel while an autonomous chain is
/// running. A chain is a sequence of agent cycles, each followed by a
/// verification run of the project's own check command.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChainEvent {
    /// A new cycle began.
    CycleStart {
        chain_id: String,
        cycle: usize,
        max_cycles: usize,
    },
    /// A cycle finished without a passing verification.
    Cycle {
        chain_id: String,
        cycle: usize,
        message: String,
        edits: usize,
    },
    /// The project's verification command ran.
    Verify {
        chain_id: String,
        cycle: usize,
        ok: bool,
        detail: String,
    },
    /// A human-readable progress line.
    Log { chain_id: String, line: String },
    /// The chain finished; `status` is passed / failed / cancelled / blocked.
    Done {
        chain_id: String,
        status: String,
        reason: String,
    },
}

/// Events emitted on the `orbit://dsh` channel while a Harness run streams.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DshEvent {
    /// A line of output from the harness.
    Line { run_id: String, line: String },
    /// The harness process exited.
    Exit { run_id: String, code: Option<i32> },
    /// The harness could not be started.
    Error { run_id: String, error: String },
}

pub const AGENT_EVENT: &str = "orbit://agent";
pub const TERMINAL_EVENT: &str = "orbit://terminal";
pub const DISCORD_EVENT: &str = "orbit://discord";
pub const CHAIN_EVENT: &str = "orbit://chain";
pub const DSH_EVENT: &str = "orbit://dsh";
