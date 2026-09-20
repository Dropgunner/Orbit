//! Serde data types shared across the Rust backend and mirrored by the
//! TypeScript IPC contract in `src/ipc/contract.ts`.

use serde::{Deserialize, Serialize};

/// User-facing model settings. The API key is optional here because it may be
/// supplied through the environment instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
            temperature: 0.2,
            api_key: None,
        }
    }
}

/// One entry in a directory listing.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_ms: Option<u64>,
}

/// A single match from a text search.
#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
}

/// An OpenAI-compatible chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A tool call attached to an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// A proposed file change, staged for the user's review before it is written
/// to disk. `before == None` means the file does not exist yet (creation).
#[derive(Debug, Clone, Serialize)]
pub struct PendingEdit {
    pub id: String,
    pub path: String,
    pub description: String,
    pub before: Option<String>,
    pub after: String,
    pub created: bool,
}

/// Repository status returned by `git status --porcelain=v1 --branch`.
#[derive(Debug, Clone, Serialize)]
pub struct GitStatus {
    pub branch: String,
    pub entries: Vec<GitStatusEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitStatusEntry {
    pub path: String,
    pub index: String,
    pub worktree: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitCommit {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date_ms: Option<u64>,
}
