import { invoke } from "@tauri-apps/api/core";
import type {
  Settings,
  FileEntry,
  SearchMatch,
  ChatMessage,
  PendingEdit,
  GitStatus,
  GitCommit,
} from "./contract";

// Typed wrappers over Tauri's `invoke`. Command names are snake_case; argument
// keys are camelCase (Tauri maps them to the snake_case Rust parameters).
export const ipc = {
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  listDir: (path: string) => invoke<FileEntry[]>("list_dir", { path }),
  readFile: (path: string) => invoke<string>("read_file", { path }),
  writeFile: (path: string, content: string) =>
    invoke<void>("write_file", { path, content }),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
  search: (root: string, query: string) =>
    invoke<SearchMatch[]>("search", { root, query }),
  spawnTerminal: (cols: number, rows: number, cwd: string | null) =>
    invoke<string>("spawn_terminal", { cols, rows, cwd }),
  writeTerminal: (id: string, data: string) =>
    invoke<void>("write_terminal", { id, data }),
  resizeTerminal: (id: string, cols: number, rows: number) =>
    invoke<void>("resize_terminal", { id, cols, rows }),
  killTerminal: (id: string) => invoke<void>("kill_terminal", { id }),
  gitStatus: (repo: string) => invoke<GitStatus>("git_status", { repo }),
  gitDiff: (repo: string, staged: boolean) =>
    invoke<string>("git_diff", { repo, staged }),
  gitStage: (repo: string, paths: string[]) =>
    invoke<void>("git_stage", { repo, paths }),
  gitUnstage: (repo: string, paths: string[]) =>
    invoke<void>("git_unstage", { repo, paths }),
  gitCommit: (repo: string, message: string) =>
    invoke<string>("git_commit", { repo, message }),
  gitLog: (repo: string, limit: number) =>
    invoke<GitCommit[]>("git_log", { repo, limit }),
  chat: (history: ChatMessage[], prompt: string, workspace: string) =>
    invoke<string>("chat", { history, prompt, workspace }),
  stopChat: (runId: string) => invoke<void>("stop_chat", { runId }),
  listPendingEdits: () => invoke<PendingEdit[]>("list_pending_edits"),
  applyPendingEdit: (id: string) =>
    invoke<void>("apply_pending_edit", { id }),
  discardPendingEdit: (id: string) =>
    invoke<void>("discard_pending_edit", { id }),
  applyAllPendingEdits: () => invoke<number>("apply_all_pending_edits"),
};
