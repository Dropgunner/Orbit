import { create } from "zustand";
import { ipc } from "../ipc/invoke";
import { isTauri } from "../lib/tauri";
import { rememberGuild } from "../lib/recents";
import {
  buildDiscussionContext,
  NARRATIVE_PROMPT,
  PROBE_PROMPT,
} from "../lib/analysisContext";
import type {
  BotPattern,
  BotReport,
  DatasetSummary,
  DiscordMessage,
  EntityFlag,
  GuardReport,
  PatternInput,
  PatternMatchReport,
  ProfileReport,
  DiscordAccount,
  DiscordChannelInfo,
  DiscordGuildInfo,
  DceStatus,
} from "../ipc/contract";
export type AnalysisKind = "profile" | "bot" | "guard";

/**
 * Monotonic run counter. Each analysis captures the current value and discards
 * its own result if a newer run started, so overlapping runs cannot interleave
 * reports with narratives.
 */
let analysisRun = 0;

/**
 * Operations that can be in flight at once. Each surface reads only its own, so
 * a long export no longer disables screening or shows "Working…" everywhere.
 */
export type BusyOp =
  | "import"
  | "connect"
  | "guild"
  | "channel"
  | "analysis"
  | "screening"
  | "patterns"
  | "export";

function addOp(busy: BusyOp[], op: BusyOp): BusyOp[] {
  return busy.includes(op) ? busy : [...busy, op];
}

function dropOp(busy: BusyOp[], op: BusyOp): BusyOp[] {
  return busy.filter((entry) => entry !== op);
}

export interface Analysis {
  kind: AnalysisKind;
  /**
   * The user these reports describe. Rendering reads the subject from here,
   * never from the live selection, so a report can never be relabelled.
   */
  subjectId: string | null;
  profile: ProfileReport | null;
  bot: BotReport | null;
  guard: GuardReport | null;
}

interface DiscordState {
  discordMode: boolean;
  dataset: DatasetSummary | null;
  messages: DiscordMessage[];
  selectedUserId: string | null;
  /** Every user highlighted in the list; `selectedUserId` is the primary one. */
  selectedUserIds: string[];
  /** Message text highlighted in the middle pane; this is what gets screened. */
  selectedMessages: string[];
  /** A screening result, shown in the Flag tab beside the button. */
  screening: GuardReport | null;
  /** Write a DeepSeek narrative automatically after each analysis. */
  autoNarrative: boolean;
  selectedChannelId: string | null;
  query: string;
  analysis: Analysis | null;
  /** Operations currently in flight; each surface reads only the ones it owns. */
  busy: BusyOp[];
  error: string | null;
  guardPolicies: string[];
  guardPolicy: string;
  dryRun: boolean;
  patterns: BotPattern[];
  patternReport: PatternMatchReport | null;
  flags: EntityFlag[];
  discussion: string;
  discussionStreaming: boolean;
  /** Which pane the in-flight DeepSeek reply belongs to. */
  discussionTarget: "chat" | "probes";
  /** DeepSeek-written probes, kept apart from the chat reply. */
  generatedProbes: string | null;
  account: DiscordAccount | null;
  guilds: DiscordGuildInfo[];
  channels: DiscordChannelInfo[];
  selectedGuildId: string | null;
  selectedLiveChannelId: string | null;
  selectedLiveChannelName: string | null;
  dceStatus: DceStatus | null;

  setDiscordMode: (open: boolean) => void;
  importFile: (path: string) => Promise<void>;
  importText: (name: string, content: string) => Promise<void>;
  selectUser: (id: string | null, additive?: boolean) => void;
  /**
   * Drop every result scoped to the current subject (user or dataset).
   * Call on any change of subject so stale reports cannot be re-attributed.
   */
  resetSubjectState: () => void;
  selectMessage: (key: string, additive?: boolean) => void;
  clearMessages: () => void;
  setAutoNarrative: (on: boolean) => void;
  runFullAnalysis: (userId: string) => Promise<void>;
  generateProbes: (userId: string) => Promise<void>;
  narrateIfEnabled: () => Promise<void>;
  selectChannel: (id: string | null) => void;
  setQuery: (q: string) => void;
  refreshMessages: () => Promise<void>;
  setGuardPolicy: (p: string) => void;
  setDryRun: (d: boolean) => void;
  runAnalysis: (kind: AnalysisKind, userId: string) => Promise<void>;
  screenMessage: (content: string, side: string) => Promise<void>;
  loadGuardPolicies: () => Promise<void>;
  loadPatterns: () => Promise<void>;
  flagPattern: (input: PatternInput) => Promise<void>;
  removePattern: (id: string) => Promise<void>;
  matchPatterns: (userId: string | null) => Promise<void>;
  loadFlags: () => Promise<void>;
  setFlag: (
    entityType: string,
    entityId: string,
    label: string,
    action: string,
    note: string
  ) => Promise<void>;
  clearFlag: (entityType: string, entityId: string) => Promise<void>;
  setDiscussion: (text: string) => void;
  appendDiscussionDelta: (text: string) => void;
  finishDiscussion: (text: string) => void;
  errorDiscussion: (error: string) => void;
  discuss: (prompt: string, context: string, target?: "chat" | "probes") => Promise<void>;
  connectDiscord: () => Promise<void>;
  selectGuild: (id: string) => Promise<void>;
  selectLiveChannel: (id: string, name: string) => void;
  fetchChannel: () => Promise<void>;
  refreshDceStatus: () => Promise<void>;
  restoreDataset: () => Promise<void>;
  dceExport: (channelId: string, format: string) => Promise<void>;
}

export const useDiscordStore = create<DiscordState>()((set, get) => ({
  discordMode: false,
  dataset: null,
  messages: [],
  selectedUserId: null,
  selectedUserIds: [],
  selectedMessages: [],
  autoNarrative: true,
  screening: null,
  selectedChannelId: null,
  query: "",
  analysis: null,
  busy: [],
  error: null,
  guardPolicies: ["strict", "permissive"],
  guardPolicy: "strict",
  dryRun: true,
  patterns: [],
  patternReport: null,
  flags: [],
  discussion: "",
  discussionStreaming: false,
  discussionTarget: "chat",
  generatedProbes: null,
  account: null,
  guilds: [],
  channels: [],
  selectedGuildId: null,
  selectedLiveChannelId: null,
  selectedLiveChannelName: null,
  dceStatus: null,

  setDiscordMode(open) {
    set({ discordMode: open });
  },

  async importFile(path) {
    set((s) => ({ busy: addOp(s.busy, "import"), error: null }));
    try {
      const dataset = await ipc.importDiscord(path);
      get().resetSubjectState();
      set({ dataset, selectedUserId: null, selectedUserIds: [], selectedChannelId: null });
      await get().refreshMessages();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set((s) => ({ busy: dropOp(s.busy, "import") }));
    }
  },

  async importText(name, content) {
    set((s) => ({ busy: addOp(s.busy, "import"), error: null }));
    try {
      const dataset = await ipc.importDiscordText(name, content);
      get().resetSubjectState();
      set({ dataset, selectedUserId: null, selectedUserIds: [], selectedChannelId: null });
      await get().refreshMessages();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set((s) => ({ busy: dropOp(s.busy, "import") }));
    }
  },

  selectUser(id, additive = false) {
    const current = get().selectedUserIds;
    let selectedUserIds: string[];
    if (id === null) {
      selectedUserIds = [];
    } else if (!additive) {
      selectedUserIds = [id];
    } else if (current.includes(id)) {
      selectedUserIds = current.filter((u) => u !== id);
    } else {
      selectedUserIds = [...current, id];
    }
    // The primary drives the analysis views; keep it inside the selection.
    const primary = id !== null && selectedUserIds.includes(id)
      ? id
      : selectedUserIds[selectedUserIds.length - 1] ?? null;
    // Reports, screenings and DeepSeek output describe the previous subject.
    get().resetSubjectState();
    set({ selectedUserId: primary, selectedUserIds });
    void get().refreshMessages();
  },

  resetSubjectState() {
    analysisRun += 1;
    set({
      analysis: null,
      screening: null,
      patternReport: null,
      discussion: "",
      generatedProbes: null,
      selectedMessages: [],
    });
  },

  selectMessage(key, additive = false) {
    const current = get().selectedMessages;
    let selectedMessages: string[];
    if (!additive) {
      selectedMessages = [key];
    } else if (current.includes(key)) {
      selectedMessages = current.filter((m) => m !== key);
    } else {
      selectedMessages = [...current, key];
    }
    set({ selectedMessages, screening: null });
  },

  clearMessages() {
    set({ selectedMessages: [], screening: null });
  },

  setAutoNarrative(on) {
    set({ autoNarrative: on });
  },

  selectChannel(id) {
    // The screening and pattern results belong to the previous message set.
    set({ selectedChannelId: id, screening: null, patternReport: null, selectedMessages: [] });
    void get().refreshMessages();
  },

  setQuery(q) {
    // Filtering is client-side over the loaded window; re-querying the backend
    // on every keystroke sent up to 1000 messages over IPC per character.
    set({ query: q });
  },

  async refreshMessages() {
    const { dataset, selectedUserId, selectedChannelId } = get();
    if (!dataset) return;
    try {
      const messages = await ipc.discordMessages(selectedUserId, selectedChannelId, null, 1000);
      set({ messages });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setGuardPolicy(p) {
    set({ guardPolicy: p });
  },

  setDryRun(d) {
    set({ dryRun: d });
  },

  async runAnalysis(kind, userId) {
    const run = ++analysisRun;
    set((s) => ({ busy: addOp(s.busy, "analysis"), error: null }));
    try {
      if (kind === "profile") {
        const profile = await ipc.analyzeProfile(userId, 200, get().dryRun);
        if (run !== analysisRun) return;
        set({ analysis: { kind, subjectId: userId, profile, bot: null, guard: null }, busy: [] });
      } else if (kind === "bot") {
        const bot = await ipc.analyzeBot(userId, 200, get().dryRun);
        if (run !== analysisRun) return;
        set({ analysis: { kind, subjectId: userId, profile: null, bot, guard: null }, busy: [] });
      } else {
        const guard = await ipc.analyzeGuard(userId, get().guardPolicy, 200, get().dryRun);
        if (run !== analysisRun) return;
        set({ analysis: { kind, subjectId: userId, profile: null, bot: null, guard }, busy: [] });
      }
      await get().narrateIfEnabled();
    } catch (e) {
      if (run !== analysisRun) return;
      set({ error: String(e), busy: [] });
    }
  },

  /** All three Jev assessments for one user, followed by the narrative. */
  async runFullAnalysis(userId) {
    // Snapshot the flags once: a report must not mix dry-run and live numbers.
    const { dryRun, guardPolicy } = get();
    const run = ++analysisRun;
    set((s) => ({ busy: addOp(s.busy, "analysis"), error: null }));
    try {
      const profile = await ipc.analyzeProfile(userId, 200, dryRun);
      const bot = await ipc.analyzeBot(userId, 200, dryRun);
      const guard = await ipc.analyzeGuard(userId, guardPolicy, 200, dryRun);
      if (run !== analysisRun) return;
      set({ analysis: { kind: "guard", subjectId: userId, profile, bot, guard }, busy: [] });
      await get().narrateIfEnabled();
    } catch (e) {
      if (run !== analysisRun) return;
      set({ error: String(e), busy: [] });
    }
  },

  /** A fresh bot report merged into the current analysis, then DeepSeek probes. */
  async generateProbes(userId) {
    const run = ++analysisRun;
    set((s) => ({ busy: addOp(s.busy, "analysis"), error: null }));
    try {
      const bot = await ipc.analyzeBot(userId, 200, get().dryRun);
      if (run !== analysisRun) return;
      // Merge rather than replace: probes are an auxiliary action and must not
      // discard the profile and guardrails already computed for this subject.
      set((s) => {
        const base = s.analysis?.subjectId === userId ? s.analysis : null;
        return {
          analysis: {
            kind: base?.kind ?? "bot",
            subjectId: userId,
            profile: base?.profile ?? null,
            bot,
            guard: base?.guard ?? null,
          },
          busy: dropOp(s.busy, "analysis"),
        };
      });
      await get().discuss(PROBE_PROMPT, buildDiscussionContext(get().analysis), "probes");
    } catch (e) {
      if (run !== analysisRun) return;
      set({ error: String(e), busy: [] });
    }
  },

  /** Write the DeepSeek narrative for the current analysis when it is enabled. */
  async narrateIfEnabled() {
    const { autoNarrative, analysis } = get();
    if (!autoNarrative || !analysis) return;
    const context = buildDiscussionContext(analysis);
    if (!context) return;
    // A new report invalidates the previous narrative. Drop it even when the
    // stream is busy, so stale prose can never sit above fresh numbers.
    set({ discussion: "" });
    await get().discuss(NARRATIVE_PROMPT, context);
  },

  async screenMessage(content, side) {
    set((s) => ({ busy: addOp(s.busy, "screening"), error: null, screening: null }));
    try {
      const screening = await ipc.screenMessage(
        content,
        side,
        get().guardPolicy,
        get().dryRun
      );
      set((s) => ({ screening, busy: dropOp(s.busy, "screening") }));
    } catch (e) {
      set({ error: String(e), busy: [] });
    }
  },

  async loadGuardPolicies() {
    try {
      const policies = await ipc.guardPolicies();
      set({
        guardPolicies: policies.policies,
        guardPolicy: policies.default,
      });
    } catch {
      /* defaults are fine */
    }
  },

  async loadPatterns() {
    try {
      const patterns = await ipc.listPatterns();
      set({ patterns });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  async flagPattern(input) {
    try {
      const patterns = await ipc.flagPattern(input);
      set({ patterns });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  async removePattern(id) {
    try {
      const patterns = await ipc.removePattern(id);
      set({ patterns });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  async matchPatterns(userId) {
    set((s) => ({ busy: addOp(s.busy, "patterns"), error: null }));
    try {
      const patternReport = await ipc.matchPatterns(userId, get().selectedChannelId);
      // A pattern report is its own card; it must not wipe the analysis.
      set((s) => ({ patternReport, busy: dropOp(s.busy, "patterns") }));
    } catch (e) {
      set({ error: String(e), busy: [] });
    }
  },

  async loadFlags() {
    try {
      const flags = await ipc.listFlags();
      set({ flags });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  async setFlag(entityType, entityId, label, action, note) {
    try {
      const flags = await ipc.setFlag(entityType, entityId, label, action, note);
      set({ flags });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  async clearFlag(entityType, entityId) {
    try {
      const flags = await ipc.clearFlag(entityType, entityId);
      set({ flags });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setDiscussion(text) {
    set({ discussion: text });
  },

  appendDiscussionDelta(text) {
    if (get().discussionTarget === "probes") {
      set((s) => ({ generatedProbes: (s.generatedProbes ?? "") + text }));
      return;
    }
    set((s) => ({ discussion: s.discussion + text }));
  },

  finishDiscussion(text) {
    set((s) =>
      s.discussionTarget === "probes"
        ? { generatedProbes: text || s.generatedProbes, discussionStreaming: false }
        : { discussion: text || s.discussion, discussionStreaming: false }
    );
  },

  errorDiscussion(error) {
    set((s) => ({
      discussionStreaming: false,
      error,
      // Never leave an empty "DeepSeek probes" card behind a failed stream.
      generatedProbes: s.discussionTarget === "probes" ? null : s.generatedProbes,
    }));
  },

  async discuss(prompt, context, target = "chat") {
    if (get().discussionStreaming) return;
    set((s) => ({
      discussionTarget: target,
      discussion: target === "chat" ? "" : s.discussion,
      generatedProbes: target === "probes" ? "" : s.generatedProbes,
      discussionStreaming: true,
      error: null,
    }));
    try {
      if (isTauri()) {
        await ipc.discuss(prompt, context);
      } else {
        // A browser has no Tauri event channel, so use the one-shot call.
        const text = await ipc.discussOnce(prompt, context);
        get().finishDiscussion(text);
      }
    } catch (e) {
      get().errorDiscussion(String(e));
    }
  },

  async connectDiscord() {
    set((s) => ({ busy: addOp(s.busy, "connect"), error: null }));
    try {
      const [account, guilds] = await Promise.all([ipc.discordWhoami(), ipc.discordGuilds()]);
      set((s) => ({ account, guilds, busy: dropOp(s.busy, "connect") }));
    } catch (e) {
      set({ error: String(e), busy: [] });
    }
  },

  async selectGuild(id) {
    rememberGuild(id);
    set((s) => ({
      busy: addOp(s.busy, "guild"),
      selectedGuildId: id,
      selectedLiveChannelId: null,
      selectedLiveChannelName: null,
      channels: [],
      screening: null,
      patternReport: null,
      selectedMessages: [],
      error: null,
    }));
    try {
      // No member fetch here. Enumerating a guild is a heavy request that
      // Discord refuses for many tokens, and its only consumer was a label;
      // nicknames arrive with the channel fetch instead.
      const channels = await ipc.discordChannels(id);
      if (get().selectedGuildId !== id) return;
      set((s) => ({ channels, busy: dropOp(s.busy, "guild") }));
    } catch (e) {
      if (get().selectedGuildId !== id) return;
      set({ error: String(e), busy: [] });
    }
  },

  selectLiveChannel(id, name) {
    set({ selectedLiveChannelId: id, selectedLiveChannelName: name });
  },

  async fetchChannel() {
    const { selectedGuildId, guilds, selectedLiveChannelId, selectedLiveChannelName } = get();
    if (!selectedLiveChannelId || !selectedLiveChannelName) return;
    const guildName = guilds.find((g) => g.id === selectedGuildId)?.name ?? "Discord server";
    set((s) => ({ busy: addOp(s.busy, "channel"), error: null }));
    try {
      const dataset = await ipc.discordFetchChannel(
        selectedGuildId,
        guildName,
        selectedLiveChannelId,
        selectedLiveChannelName,
        1000
      );
      get().resetSubjectState();
      set((s) => ({ dataset, selectedUserId: null, selectedUserIds: [], selectedChannelId: null, busy: dropOp(s.busy, "channel") }));
      await get().refreshMessages();
    } catch (e) {
      set({ error: String(e), busy: [] });
    }
  },

  async refreshDceStatus() {
    try {
      const dceStatus = await ipc.discordDceStatus();
      set({ dceStatus });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  /** Reload the dataset the backend still holds, so a reload is not a reset. */
  async restoreDataset() {
    if (get().dataset) return;
    try {
      const dataset = await ipc.discordSummary();
      set({ dataset });
      await get().refreshMessages();
    } catch {
      /* nothing imported yet; the panel shows its empty state */
    }
  },

  async dceExport(channelId, format) {
    set((s) => ({ busy: addOp(s.busy, "export"), error: null }));
    try {
      const result = await ipc.discordDceExport(channelId, format, true);
      if (result.dataset) {
        get().resetSubjectState();
        set({ dataset: result.dataset, selectedUserId: null, selectedUserIds: [], selectedChannelId: null });
        await get().refreshMessages();
      }
      set({ busy: [] });
    } catch (e) {
      set({ error: String(e), busy: [] });
    }
  },
}));
