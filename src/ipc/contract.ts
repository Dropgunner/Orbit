// TypeScript mirror of the Rust serde types in src-tauri/src/types.rs and
// src-tauri/src/events.rs. Keep these in sync when the backend changes.

export interface Settings {
  base_url: string;
  model: string;
  temperature: number;
  api_key: string | null;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified_ms: number | null;
}

export interface SearchMatch {
  path: string;
  line_number: number;
  line: string;
}

export interface ChatMessage {
  role: string;
  content: string;
  tool_calls?: ToolCall[] | null;
  tool_call_id?: string | null;
  name?: string | null;
}

export interface ToolCall {
  id: string;
  type: string;
  function: { name: string; arguments: string };
}

export interface PendingEdit {
  id: string;
  path: string;
  description: string;
  before: string | null;
  after: string;
  created: boolean;
}

export interface GitStatus {
  branch: string;
  entries: GitStatusEntry[];
}

export interface GitStatusEntry {
  path: string;
  index: string;
  worktree: string;
}

export interface GitCommit {
  hash: string;
  subject: string;
  author: string;
  date_ms: number | null;
}

// ---- events (serde tag = "type", rename_all = "snake_case") ----

export type AgentEvent =
  | { type: "delta"; run_id: string; content: string }
  | { type: "tool_call"; run_id: string; id: string; name: string; arguments: string }
  | { type: "tool_result"; run_id: string; id: string; name: string; result: string }
  | { type: "pending_edit"; run_id: string; edit: PendingEdit }
  | { type: "done"; run_id: string; message: string }
  | { type: "error"; run_id: string; error: string };

export type TerminalEvent =
  | { type: "data"; id: string; data: string }
  | { type: "exit"; id: string; code: number | null };

export const AGENT_EVENT = "orbit://agent";
export const TERMINAL_EVENT = "orbit://terminal";
