//! Agent tool definitions and executors. Tools let the model inspect the
//! workspace, propose file edits (staged for review), and run shell commands.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::events::{AgentEvent, AGENT_EVENT};
use crate::model::{FunctionDef, ToolDef};
use crate::state::AppState;
use crate::types::{FileEntry, PendingEdit, SearchMatch};

/// Directories skipped during recursive search and listing.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
];

/// Maximum characters returned by read_file / run_command so a single tool
/// result cannot blow up the model context.
const MAX_READ_CHARS: usize = 200_000;
const MAX_COMMAND_CHARS: usize = 80_000;
const MAX_SEARCH_MATCHES: usize = 200;

fn object(properties: Value, required: &[&str]) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn string_prop(description: &str) -> Value {
    serde_json::json!({ "type": "string", "description": description })
}

/// The tool schemas exposed to the model.
pub fn tool_definitions() -> Vec<ToolDef> {
    vec![
        tool(
            "list_directory",
            "List the entries of a directory inside the workspace. Omit path for the workspace root.",
            object(
                serde_json::json!({
                    "path": string_prop("Directory path, relative to the workspace root or absolute. Optional.")
                }),
                &[],
            ),
        ),
        tool(
            "read_file",
            "Read a text file and return its contents.",
            object(
                serde_json::json!({
                    "path": string_prop("File path, relative to the workspace root or absolute.")
                }),
                &["path"],
            ),
        ),
        tool(
            "search",
            "Search file contents in the workspace for a query string (case-insensitive).",
            object(
                serde_json::json!({
                    "query": string_prop("Substring to search for.")
                }),
                &["query"],
            ),
        ),
        tool(
            "write_file",
            "Create a new file or replace an existing file with the given content. The change is staged for user review before it is written to disk.",
            object(
                serde_json::json!({
                    "path": string_prop("File path, relative to the workspace root or absolute."),
                    "content": string_prop("Complete new content of the file.")
                }),
                &["path", "content"],
            ),
        ),
        tool(
            "edit_file",
            "Make a surgical edit by replacing a unique old_string with new_string inside a file. The change is staged for user review. Prefer this over write_file for small edits; old_string must occur exactly once.",
            object(
                serde_json::json!({
                    "path": string_prop("File path, relative to the workspace root or absolute."),
                    "old_string": string_prop("Exact text to replace; must be unique in the file."),
                    "new_string": string_prop("Replacement text.")
                }),
                &["path", "old_string", "new_string"],
            ),
        ),
        tool(
            "run_command",
            "Run a shell command in the workspace and return its combined output. Use for build, test, lint, and git commands.",
            object(
                serde_json::json!({
                    "command": string_prop("Shell command to run.")
                }),
                &["command"],
            ),
        ),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> ToolDef {
    ToolDef {
        def_type: "function".to_string(),
        function: FunctionDef {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
    }
}

/// Executes a tool and returns a string result for the model.
pub async fn execute_tool(
    app: &AppHandle,
    state: &AppState,
    run_id: &str,
    name: &str,
    args: &Value,
    workspace: &Path,
) -> Result<String, String> {
    match name {
        "list_directory" => list_directory(workspace, args).await,
        "read_file" => read_file(workspace, args).await,
        "search" => search(workspace, args).await,
        "write_file" => write_file(app, state, run_id, workspace, args),
        "edit_file" => edit_file(app, state, run_id, workspace, args),
        "run_command" => run_command(workspace, args).await,
        other => Err(format!("unknown tool: {other}")),
    }
}

fn arg_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing string argument: {key}"))
}

fn arg_string_optional(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Lexically normalizes a path (resolving `.` and `..`) without touching the
/// filesystem, so non-existent paths can be validated too.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves `raw` against the workspace and rejects paths that escape it.
fn resolve_in_workspace(workspace: &Path, raw: &str) -> Result<PathBuf, String> {
    let ws = workspace
        .canonicalize()
        .map_err(|e| format!("workspace error: {e}"))?;
    let candidate = Path::new(raw);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        ws.join(candidate)
    };
    let normalized = normalize(&joined);
    if !normalized.starts_with(&ws) {
        return Err(format!("path {raw} escapes the workspace"));
    }
    Ok(normalized)
}

async fn list_directory(workspace: &Path, args: &Value) -> Result<String, String> {
    let raw = arg_string_optional(args, "path").unwrap_or_default();
    let dir = resolve_in_workspace(workspace, &raw)?;
    let mut entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("read_dir failed: {e}"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            Some(FileEntry {
                is_dir: metadata.is_dir(),
                name,
                path: path.to_string_lossy().to_string(),
                size: metadata.len(),
                modified_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64),
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    if entries.is_empty() {
        return Ok("(empty directory)".to_string());
    }
    let mut out = String::new();
    for entry in entries {
        if entry.is_dir {
            out.push_str(&format!("[dir]  {}\n", entry.name));
        } else {
            out.push_str(&format!("[file] {} ({} bytes)\n", entry.name, entry.size));
        }
    }
    Ok(out)
}

async fn read_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let raw = arg_string(args, "path")?;
    let path = resolve_in_workspace(workspace, &raw)?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read failed for {}: {e}", path.display()))?;
    Ok(truncate(&content, MAX_READ_CHARS))
}

pub(crate) fn search_sync(workspace: &Path, query: &str) -> Result<Vec<SearchMatch>, String> {
    let mut matches: Vec<SearchMatch> = Vec::new();
    let lower = query.to_lowercase();
    let mut stack = vec![workspace.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(path);
                }
                continue;
            }
            if name.starts_with('.') {
                continue;
            }
            if matches.len() >= MAX_SEARCH_MATCHES {
                return Ok(matches);
            }
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if metadata.len() > 1_000_000 {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&lower) {
                    matches.push(SearchMatch {
                        path: path.to_string_lossy().to_string(),
                        line_number: i + 1,
                        line: line.trim().to_string(),
                    });
                    if matches.len() >= MAX_SEARCH_MATCHES {
                        return Ok(matches);
                    }
                }
            }
        }
    }
    Ok(matches)
}

async fn search(workspace: &Path, args: &Value) -> Result<String, String> {
    let query = arg_string(args, "query")?;
    let ws = workspace.to_path_buf();
    let query_for_task = query.clone();
    let matches = tokio::task::spawn_blocking(move || search_sync(&ws, &query_for_task))
        .await
        .map_err(|e| format!("search join error: {e}"))??;
    if matches.is_empty() {
        return Ok(format!("No matches for {query}."));
    }
    let mut out = String::new();
    for m in matches {
        out.push_str(&format!("{}:{}: {}\n", m.path, m.line_number, m.line));
    }
    Ok(out)
}

fn stage_edit(
    app: &AppHandle,
    state: &AppState,
    run_id: &str,
    path: PathBuf,
    before: Option<String>,
    after: String,
    description: String,
) -> PendingEdit {
    let id = {
        let mut counter = state.next_edit_id.lock().unwrap();
        let id = format!("edit-{}", *counter);
        *counter += 1;
        id
    };
    let edit = PendingEdit {
        id,
        path: path.to_string_lossy().to_string(),
        description,
        created: before.is_none(),
        before,
        after,
    };
    state.pending_edits.lock().unwrap().insert(edit.id.clone(), edit.clone());
    let _ = app.emit(
        AGENT_EVENT,
        AgentEvent::PendingEdit {
            run_id: run_id.to_string(),
            edit: edit.clone(),
        },
    );
    edit
}

fn write_file(
    app: &AppHandle,
    state: &AppState,
    run_id: &str,
    workspace: &Path,
    args: &Value,
) -> Result<String, String> {
    let raw = arg_string(args, "path")?;
    let content = arg_string(args, "content")?;
    let path = resolve_in_workspace(workspace, &raw)?;
    let before = std::fs::read_to_string(&path).ok();
    let kind = if before.is_none() { "create" } else { "rewrite" };
    let edit = stage_edit(
        app,
        state,
        run_id,
        path.clone(),
        before,
        content,
        format!("{kind} {}", path.display()),
    );
    Ok(format!(
        "Staged {} of {} for review (edit id {}). It is not written to disk until the user approves.",
        kind,
        path.display(),
        edit.id
    ))
}

fn edit_file(
    app: &AppHandle,
    state: &AppState,
    run_id: &str,
    workspace: &Path,
    args: &Value,
) -> Result<String, String> {
    let raw = arg_string(args, "path")?;
    let old_string = arg_string(args, "old_string")?;
    let new_string = arg_string(args, "new_string")?;
    let path = resolve_in_workspace(workspace, &raw)?;
    let original = std::fs::read_to_string(&path)
        .map_err(|e| format!("read failed for {}: {e}", path.display()))?;

    let occurrences = original.matches(&old_string).count();
    if occurrences == 0 {
        return Err(format!(
            "old_string was not found in {}",
            path.display()
        ));
    }
    if occurrences > 1 {
        return Err(format!(
            "old_string occurs {occurrences} times in {}; provide more surrounding context to make it unique",
            path.display()
        ));
    }

    let updated = original.replacen(&old_string, &new_string, 1);
    let edit = stage_edit(
        app,
        state,
        run_id,
        path.clone(),
        Some(original),
        updated,
        format!("edit {}", path.display()),
    );
    Ok(format!(
        "Staged edit of {} for review (edit id {}). It is not written to disk until the user approves.",
        path.display(),
        edit.id
    ))
}

async fn run_command(workspace: &Path, args: &Value) -> Result<String, String> {
    let command = arg_string(args, "command")?;
    let ws = workspace.to_path_buf();
    let output = tokio::time::timeout(
        Duration::from_secs(120),
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(&ws)
            .env("TERM", "dumb")
            .env("CI", "1")
            .output(),
    )
    .await
    .map_err(|_| "command timed out after 120s".to_string())?
    .map_err(|e| format!("failed to run command: {e}"))?;

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    let code = output
        .status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "killed".to_string());
    if text.is_empty() {
        text = format!("(no output, exit code {code})");
    } else {
        text = format!("{}\n(exit code {code})", truncate(&text, MAX_COMMAND_CHARS));
    }
    Ok(text)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}\n... [truncated]")
}
