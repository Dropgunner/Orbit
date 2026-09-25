import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { isTauri, webToken } from "../lib/tauri";
import type {
  Settings,
  FileEntry,
  SearchMatch,
  ChatMessage,
  PendingEdit,
  GitStatus,
  GitCommit,
  DatasetSummary,
  DiscordMessage,
  ProfileReport,
  BotReport,
  GuardReport,
  BotPattern,
  PatternInput,
  PatternMatchReport,
  EntityFlag,
  GuardPolicies,
  DiscordAccount,
  DiscordGuildInfo,
  DiscordChannelInfo,
  MemberRoster,
  DceStatus,
  DceExportResult,
  ClassifyResult,
  ScoreResult,
  PsychProfileResult,
  NoulResult,
  PruneResult,
  CompactResult,
  CompactEntry,
  GenerateResult,
  WorkspaceMode,
  ChainRun,
  DshStatus,
} from "./contract";

// Typed wrappers over Tauri's `invoke`. Command names are snake_case; argument
// keys are camelCase (Tauri maps them to the snake_case Rust parameters).
//
// In a browser the same calls go to the app's local web server, which bridges
// them to the identical Rust command functions.
async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    return tauriInvoke<T>(command, args);
  }
  const response = await fetch("/api/invoke", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-orbit-token": webToken(),
    },
    body: JSON.stringify({ command, args: args ?? {} }),
  });
  const payload = (await response.json().catch(() => null)) as
    | { ok: true; value: T }
    | { ok: false; error: string }
    | null;
  if (!response.ok || !payload || !payload.ok) {
    throw new Error(payload && !payload.ok ? payload.error : `HTTP ${response.status}`);
  }
  return payload.value;
}

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

  // ---- self-hosting, chains, and the DeepSeek Harness ----
  /** Reports whether this workspace is the repository that builds Orbit. */
  workspaceMode: (workspace: string) =>
    invoke<WorkspaceMode>("workspace_mode", { workspace }),
  /** Starts a chained autonomous run: edit, verify, repeat. */
  startChain: (goal: string, workspace: string, maxCycles: number | null) =>
    invoke<string>("start_chain", { goal, workspace, maxCycles }),
  stopChain: (chainId: string) => invoke<void>("stop_chain", { chainId }),
  getChain: (chainId: string | null) =>
    invoke<ChainRun[]>("get_chain", { chainId }),
  /** Reports whether the Harness CLI (`dsh`) is installed. */
  dshStatus: () => invoke<DshStatus>("dsh_status"),
  /** Hands one task to the Harness one-shot profile; output streams on `orbit://dsh`. */
  dshRun: (prompt: string, workspace: string) =>
    invoke<string>("dsh_run", { prompt, workspace }),
  stopDsh: (runId: string) => invoke<void>("stop_dsh", { runId }),

  importDiscord: (path: string) => invoke<DatasetSummary>("import_discord", { path }),
  importDiscordText: (name: string, content: string) =>
    invoke<DatasetSummary>("import_discord_text", { name, content }),
  discordSummary: () => invoke<DatasetSummary>("discord_summary"),
  discordMessages: (
    userId: string | null,
    channelId: string | null,
    query: string | null,
    limit: number | null
  ) =>
    invoke<DiscordMessage[]>("discord_messages", {
      userId,
      channelId,
      query,
      limit,
    }),
  analyzeProfile: (userId: string, limit: number | null, dryRun: boolean) =>
    invoke<ProfileReport>("analyze_profile", { userId, limit, dryRun }),
  analyzeBot: (userId: string, limit: number | null, dryRun: boolean) =>
    invoke<BotReport>("analyze_bot", { userId, limit, dryRun }),
  analyzeGuard: (
    userId: string,
    policy: string | null,
    limit: number | null,
    dryRun: boolean
  ) => invoke<GuardReport>("analyze_guard", { userId, policy, limit, dryRun }),
  screenMessage: (content: string, side: string | null, policy: string | null, dryRun: boolean) =>
    invoke<GuardReport>("screen_message_cmd", { content, side, policy, dryRun }),
  guardPolicies: () => invoke<GuardPolicies>("guard_policies"),

  // ---- Jev (TypeSafe) toolkit ----
  classify: (text: string, instructions: string, labels: string[]) =>
    invoke<ClassifyResult>("classify_cmd", { text, instructions, labels }),
  score: (text: string, instructions: string, rubric: string[]) =>
    invoke<ScoreResult>("score_cmd", { text, instructions, rubric }),
  psychProfile: (text: string) =>
    invoke<PsychProfileResult>("psych_profile_cmd", { text }),
  noul: (text: string, instructions: string, yes: string | null, no: string | null) =>
    invoke<NoulResult>("noul_cmd", { text, instructions, yes, no }),
  prune: (text: string, goal: string | null, chunkLines: number | null, keepThreshold: number | null) =>
    invoke<PruneResult>("prune_cmd", { text, goal, chunkLines, keepThreshold }),
  compact: (entries: CompactEntry[], goal: string | null, preserveRecent: number | null, keepThreshold: number | null) =>
    invoke<CompactResult>("compact_cmd", { entries, goal, preserveRecent, keepThreshold }),
  generate: (prompt: string, candidates: string[], maxTokens: number | null, stop: string[] | null) =>
    invoke<GenerateResult>("generate_cmd", { prompt, candidates, maxTokens, stop }),
  listPatterns: () => invoke<BotPattern[]>("list_patterns"),
  flagPattern: (input: PatternInput) =>
    invoke<BotPattern[]>("flag_pattern", { input }),
  removePattern: (id: string) => invoke<BotPattern[]>("remove_pattern", { id }),
  matchPatterns: (userId: string | null, channelId: string | null) =>
    invoke<PatternMatchReport>("match_patterns", { userId, channelId }),
  listFlags: () => invoke<EntityFlag[]>("list_flags"),
  setFlag: (
    entityType: string,
    entityId: string,
    label: string,
    action: string,
    note: string
  ) =>
    invoke<EntityFlag[]>("set_flag", {
      entityType,
      entityId,
      label,
      action,
      note,
    }),
  clearFlag: (entityType: string, entityId: string) =>
    invoke<EntityFlag[]>("clear_flag", { entityType, entityId }),
  discuss: (prompt: string, context: string) =>
    invoke<string>("discuss", { prompt, context }),

  discordWhoami: () => invoke<DiscordAccount>("discord_whoami"),
  discordGuilds: () => invoke<DiscordGuildInfo[]>("discord_guilds"),
  discordChannels: (guildId: string) =>
    invoke<DiscordChannelInfo[]>("discord_channels", { guildId }),
  discordMembers: (guildId: string, limit: number | null) =>
    invoke<MemberRoster>("discord_members", { guildId, limit }),
  discordFetchChannel: (
    guildId: string | null,
    guildName: string,
    channelId: string,
    channelName: string,
    limit: number | null
  ) =>
    invoke<DatasetSummary>("discord_fetch_channel", {
      guildId,
      guildName,
      channelId,
      channelName,
      limit,
    }),
  discordDceStatus: () => invoke<DceStatus>("discord_dce_status"),
  discordDceExport: (
    channelId: string,
    format: string | null,
    loadIntoApp: boolean
  ) => invoke<DceExportResult>("discord_dce_export", { channelId, format, loadIntoApp }),
  discordExportJson: (path: string) =>
    invoke<string>("discord_export_json", { path }),
  discordExportJsonText: () =>
    invoke<unknown>("discord_export_json_text"),
  discussOnce: (prompt: string, context: string) =>
    invoke<string>("discuss_once", { prompt, context }),
  startWebUi: () => invoke<string>("start_web_ui"),
  stopWebUi: () => invoke<void>("stop_web_ui"),
  webUiUrl: () => invoke<string | null>("web_ui_url"),
  openDiscordWeb: () => invoke<void>("open_discord_web"),
  discordWebOpen: () => invoke<boolean>("discord_web_open"),
};
