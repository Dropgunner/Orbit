// TypeScript mirror of the Rust serde types in src-tauri/src/types.rs and
// src-tauri/src/events.rs. Keep these in sync when the backend changes.

export interface Settings {
  base_url: string;
  model: string;
  temperature: number;
  api_key: string | null;
  typesafe_api_key: string | null;
  typesafe_base_url: string;
  typesafe_model: string;
  discord_token: string | null;
  /** Explicit path to the DeepSeek Harness CLI; empty means "look on PATH". */
  dsh_path?: string | null;
  /**
   * Set by the local web bridge, which never discloses secrets. True means a
   * secret is stored; the value itself stays null in the browser.
   */
  api_key_set?: boolean;
  typesafe_api_key_set?: boolean;
  discord_token_set?: boolean;
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
export const DISCORD_EVENT = "orbit://discord";
export const CHAIN_EVENT = "orbit://chain";
export const DSH_EVENT = "orbit://dsh";

// ---- Discord analysis mode ----

export interface DiscordGuild {
  id: string | null;
  name: string;
}

export interface DiscordChannel {
  id: string | null;
  name: string;
  category: string | null;
}

export interface DiscordUser {
  id: string;
  name: string;
  discriminator: string | null;
  nickname: string | null;
  is_bot: boolean;
  message_count: number;
}

export interface DiscordMessage {
  id: string | null;
  channel_id: string | null;
  author_id: string;
  timestamp: string | null;
  content: string;
}

export interface DatasetSummary {
  guild: DiscordGuild;
  channels: DiscordChannel[];
  users: DiscordUser[];
  message_count: number;
}

export interface Usage {
  input_tokens: number;
  output_tokens: number;
}

export interface EvidenceExcerpt {
  index: number;
  excerpt: string;
}

export interface PerMessageScore {
  index: number;
  score: number;
  confidence: number;
}

export interface DimensionSummary {
  id: string;
  label: string;
  category: string;
  group: string;
  polarity: string;
  direction: string;
  basis: string;
  note?: string | null;
  score: number;
  stddev: number;
  confidence: number;
  n: number;
  stable: boolean;
  evidence: EvidenceExcerpt[];
  messages: PerMessageScore[];
}

export interface ProfileReport {
  source: string;
  author: string | null;
  message_count: number;
  scored_count: number;
  generated_at: string;
  dimensions: DimensionSummary[];
  usage: Usage;
}

export interface BotDimensionSummary {
  id: string;
  label: string;
  category: string;
  group: string;
  polarity: string;
  direction: string;
  basis: string;
  weight: number;
  note?: string | null;
  score: number;
  stddev: number;
  confidence: number;
  n: number;
  stable: boolean;
  evidence: EvidenceExcerpt[];
  messages: PerMessageScore[];
}

export interface BotProbe {
  dimension: string;
  label: string;
  items: string[];
}

export interface BotReport {
  source: string;
  turn_count: number;
  scored_count: number;
  generated_at: string;
  likelihood: number;
  verdict: string;
  dimensions: BotDimensionSummary[];
  probes: BotProbe[];
  usage: Usage;
}

export type RiskAction = "pass" | "review" | "block" | "support";

export interface GuardHazard {
  id: string;
  label: string;
  kind: string;
  value: number;
  action: string;
  triggered: string | null;
}

export interface GuardReport {
  source: string;
  policy: string;
  action: RiskAction;
  severity: number;
  hazards: GuardHazard[];
  flagged: string[];
  generated_at: string;
  usage: Usage;
}

export type PatternKind = "regex" | "substring" | "word";

export interface BotPattern {
  id: string;
  label: string;
  creator: string;
  category: string;
  kind: PatternKind;
  pattern: string;
  regexFlags: string;
  caseSensitive: boolean;
  note: string;
  createdAt: string;
  updatedAt: string;
}

export interface PatternInput {
  id?: string | null;
  label: string;
  creator: string;
  category: string;
  kind: PatternKind;
  pattern: string;
  regexFlags?: string | null;
  caseSensitive?: boolean | null;
  note?: string | null;
}

export interface PatternExample {
  index: number;
  timestamp: string | null;
  match: string;
  excerpt: string;
}

export interface PatternMatchSummary {
  patternId: string;
  label: string;
  creator: string;
  category: string;
  kind: PatternKind;
  pattern: string;
  matches: number;
  firstIndex: number | null;
  lastIndex: number | null;
  examples: PatternExample[];
}

export interface TimeBucket {
  start: string;
  count: number;
}

export interface GapStats {
  matchCount: number;
  bucketSpanMs: number;
  meanGapMs: number;
  medianGapMs: number;
  minGapMs: number;
  maxGapMs: number;
  stddevGapMs: number;
  coefficientOfVariation: number;
}

export interface PatternMatchReport {
  source: string;
  generatedAt: string;
  messageCount: number;
  patternsScanned: number;
  matchedPatterns: number;
  matchedMessages: number;
  totalMatches: number;
  matchedRate: number;
  patterns: PatternMatchSummary[];
  timeSeries: TimeBucket[];
  gapStats: GapStats | null;
}

export interface EntityFlag {
  entity_type: string;
  entity_id: string;
  label: string;
  action: string;
  note: string;
  created_at: string;
}

export interface GuardPolicies {
  policies: string[];
  default: string;
}

export type DiscordEvent =
  | { type: "delta"; run_id: string; content: string }
  | { type: "done"; run_id: string; message: string }
  | { type: "error"; run_id: string; error: string };

// ---- Live Discord (native client + DiscordChatExporter) ----

export interface DiscordAccount {
  id: string;
  username: string;
  global_name: string | null;
}

export interface DiscordGuildInfo {
  id: string;
  name: string;
}

export interface DiscordChannelInfo {
  id: string;
  name: string;
}

export interface MemberRoster {
  users: DiscordUser[];
  unavailable: boolean;
  reason: string | null;
}

export interface DceStatus {
  installed: boolean;
  version: string | null;
  path: string | null;
}

export interface DceExportResult {
  path: string;
  loaded: boolean;
  dataset: DatasetSummary | null;
}

// ---- Jev (TypeSafe) toolkit ----

export interface ClassifyProbability {
  label: string;
  probability: number;
}

export interface ClassifyResult {
  model: string;
  usage: Usage;
  choice: string;
  confidence: number;
  probabilities: ClassifyProbability[];
}

export interface ScoreProbability {
  level: number;
  description: string;
  probability: number;
}

export interface ScoreResult {
  model: string;
  usage: Usage;
  score: number;
  confidence: number;
  probabilities: ScoreProbability[];
}

export interface PsychDimension {
  id: string;
  category: string;
  label: string;
  polarity: string;
  direction: string;
  score: number;
  confidence: number;
  note: string;
}

export interface PsychProfileResult {
  model: string;
  usage: Usage;
  dimensions: PsychDimension[];
}

export interface NoulResult {
  model: string;
  usage: Usage;
  yes_probability: number;
}

export interface PruneResult {
  model: string;
  usage: Usage;
  pruned: string;
  kept_chunks: number;
  dropped_chunks: number;
  omitted_lines: number;
}

export interface CompactEntry {
  id: string;
  text: string;
  result: string | null;
}

export interface CompactDecision {
  id: string;
  keep_call: boolean;
  keep_result: boolean;
  action: string;
}

export interface CompactStats {
  before: number;
  after: number;
  dropped: number;
  truncated: number;
}

export interface CompactResult {
  model: string;
  usage: Usage;
  entries: CompactEntry[];
  decisions: CompactDecision[];
  stats: CompactStats;
}

export interface GenerateResult {
  model: string;
  usage: Usage;
  text: string;
  tokens: string[];
}

// ---------------------------------------------------------------------------
// Self-hosting, autonomous chains, and the DeepSeek Harness
// ---------------------------------------------------------------------------

/** One command that proves a change is good, e.g. a typecheck or cargo test. */
export interface VerifyStep {
  label: string;
  command: string;
}

/** Whether the agent's workspace is the repository that builds Orbit. */
export interface WorkspaceMode {
  self_hosting: boolean;
  project: string | null;
  verify_commands: VerifyStep[];
  has_profiler_cli: boolean;
  has_plugin: boolean;
  note: string;
}

/** How a chain ended. */
export type ChainStatus =
  | "running"
  | "passed"
  | "failed"
  | "cancelled"
  | "blocked";

/** An autonomous chain: repeated agent cycles, each followed by verification. */
export interface ChainRun {
  id: string;
  goal: string;
  cycle: number;
  max_cycles: number;
  status: ChainStatus;
  log: string[];
}

/** Whether the DeepSeek Harness CLI can be driven from this machine. */
export interface DshStatus {
  available: boolean;
  program: string | null;
  profile: string;
  note: string;
}

/**
 * Events on the `orbit://chain` channel. Tagged by `type`, mirroring the Rust
 * `ChainEvent` enum.
 */
export type ChainEvent =
  | { type: "cycle_start"; chain_id: string; cycle: number; max_cycles: number }
  | { type: "cycle"; chain_id: string; cycle: number; message: string; edits: number }
  | { type: "verify"; chain_id: string; cycle: number; ok: boolean; detail: string }
  | { type: "log"; chain_id: string; line: string }
  | { type: "done"; chain_id: string; status: ChainStatus; reason: string };

/** Events on the `orbit://dsh` channel, mirroring the Rust `DshEvent` enum. */
export type DshEvent =
  | { type: "line"; run_id: string; line: string }
  | { type: "exit"; run_id: string; code: number | null }
  | { type: "error"; run_id: string; error: string };
