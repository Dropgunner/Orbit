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

pub const AGENT_EVENT: &str = "orbit://agent";
pub const TERMINAL_EVENT: &str = "orbit://terminal";
