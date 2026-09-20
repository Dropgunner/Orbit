import { create } from "zustand";
import { ipc } from "../ipc/invoke";
import type {
  ChatMessage,
  GitCommit,
  GitStatus,
  PendingEdit,
  Settings,
} from "../ipc/contract";

export interface OpenFile {
  path: string;
  content: string;
  savedContent: string;
}

export interface ChatMsg {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  toolName?: string;
  toolArgs?: string;
  toolResult?: string;
  toolCallId?: string;
  streaming?: boolean;
}

export type GitTab = "tree" | "git";

interface StoreState {
  projectRoot: string | null;
  openFiles: OpenFile[];
  activePath: string | null;
  chat: ChatMsg[];
  pendingEdits: PendingEdit[];
  gitStatus: GitStatus | null;
  gitDiff: string;
  gitLog: GitCommit[];
  gitTab: GitTab;
  gitLoading: boolean;
  settings: Settings | null;
  settingsOpen: boolean;
  streaming: boolean;
  currentRunId: string | null;
  terminalIds: string[];
  activeTerminal: string | null;
  statusMessage: string;

  openProject: (root: string) => Promise<void>;
  openFile: (path: string) => Promise<void>;
  setActivePath: (path: string) => void;
  updateFileContent: (path: string, content: string) => void;
  saveFile: (path: string) => Promise<void>;
  closeFile: (path: string) => void;
  reloadOpenFile: (path: string) => Promise<void>;
  setGitTab: (tab: GitTab) => void;
  refreshGit: () => Promise<void>;
  stagePaths: (paths: string[]) => Promise<void>;
  unstagePaths: (paths: string[]) => Promise<void>;
  commit: (message: string) => Promise<void>;
  refreshPendingEdits: () => Promise<void>;
  applyEdit: (id: string) => Promise<void>;
  discardEdit: (id: string) => Promise<void>;
  applyAllEdits: () => Promise<void>;
  loadSettings: () => Promise<void>;
  saveSettings: (settings: Settings) => Promise<void>;
  setSettingsOpen: (open: boolean) => void;
  sendChat: (prompt: string) => Promise<void>;
  stopChat: () => Promise<void>;
  addPendingEdit: (edit: PendingEdit) => void;
  appendStreamingDelta: (content: string) => void;
  appendToolCall: (callId: string, name: string, args: string) => void;
  appendToolResult: (callId: string, name: string, result: string) => void;
  finishStreaming: (message: string) => void;
  finishStreamingError: (error: string) => void;
  addTerminal: (id: string) => void;
  removeTerminal: (id: string) => void;
  setActiveTerminal: (id: string | null) => void;
  setStatusMessage: (message: string) => void;
}

let idCounter = 0;
function nextId(prefix: string): string {
  idCounter += 1;
  return prefix + "-" + idCounter + "-" + Date.now();
}

export const useStore = create<StoreState>()((set, get) => ({
  projectRoot: null,
  openFiles: [],
  activePath: null,
  chat: [],
  pendingEdits: [],
  gitStatus: null,
  gitDiff: "",
  gitLog: [],
  gitTab: "tree",
  gitLoading: false,
  settings: null,
  settingsOpen: false,
  streaming: false,
  currentRunId: null,
  terminalIds: [],
  activeTerminal: null,
  statusMessage: "Ready",

  async openProject(root) {
    set({ projectRoot: root, statusMessage: "Opened " + root });
    const store = get();
    void store.refreshGit();
    void store.refreshPendingEdits();
    void store.loadSettings();
    try {
      const id = await ipc.spawnTerminal(80, 24, root);
      set({ terminalIds: [id], activeTerminal: id });
    } catch {
      // Terminal is optional; the project still opens without it.
    }
  },

  async openFile(path) {
    const existing = get().openFiles.find((f) => f.path === path);
    if (existing) {
      set({ activePath: path });
      return;
    }
    try {
      const content = await ipc.readFile(path);
      set((s) => ({
        openFiles: [...s.openFiles, { path, content, savedContent: content }],
        activePath: path,
      }));
    } catch (e) {
      set({ statusMessage: "Failed to open " + path + ": " + String(e) });
    }
  },

  setActivePath(path) {
    set({ activePath: path });
  },

  updateFileContent(path, content) {
    set((s) => ({
      openFiles: s.openFiles.map((f) =>
        f.path === path ? { ...f, content } : f
      ),
    }));
  },

  async saveFile(path) {
    const file = get().openFiles.find((f) => f.path === path);
    if (!file) return;
    try {
      await ipc.writeFile(path, file.content);
      set((s) => ({
        openFiles: s.openFiles.map((f) =>
          f.path === path ? { ...f, savedContent: f.content } : f
        ),
        statusMessage: "Saved " + path,
      }));
      void get().refreshGit();
    } catch (e) {
      set({ statusMessage: "Failed to save: " + String(e) });
    }
  },

  closeFile(path) {
    set((s) => {
      const openFiles = s.openFiles.filter((f) => f.path !== path);
      const activePath =
        s.activePath === path
          ? (openFiles[openFiles.length - 1]?.path ?? null)
          : s.activePath;
      return { openFiles, activePath };
    });
  },

  async reloadOpenFile(path) {
    const file = get().openFiles.find((f) => f.path === path);
    if (!file) return;
    try {
      const content = await ipc.readFile(path);
      set((s) => ({
        openFiles: s.openFiles.map((f) =>
          f.path === path ? { ...f, content, savedContent: content } : f
        ),
      }));
    } catch {
      // File may have been deleted; leave the buffer as-is.
    }
  },

  setGitTab(tab) {
    set({ gitTab: tab });
    if (tab === "git") void get().refreshGit();
  },

  async refreshGit() {
    const root = get().projectRoot;
    if (!root) return;
    set({ gitLoading: true });
    try {
      const [status, diff, log] = await Promise.all([
        ipc.gitStatus(root),
        ipc.gitDiff(root, false),
        ipc.gitLog(root, 50),
      ]);
      set({ gitStatus: status, gitDiff: diff, gitLog: log });
    } catch {
      set({ gitStatus: null, gitDiff: "", gitLog: [] });
    } finally {
      set({ gitLoading: false });
    }
  },

  async stagePaths(paths) {
    const root = get().projectRoot;
    if (!root) return;
    await ipc.gitStage(root, paths);
    await get().refreshGit();
  },

  async unstagePaths(paths) {
    const root = get().projectRoot;
    if (!root) return;
    await ipc.gitUnstage(root, paths);
    await get().refreshGit();
  },

  async commit(message) {
    const root = get().projectRoot;
    if (!root) return;
    try {
      const hash = await ipc.gitCommit(root, message);
      set({ statusMessage: "Committed " + hash.slice(0, 7) });
      await get().refreshGit();
    } catch (e) {
      set({ statusMessage: "Commit failed: " + String(e) });
      throw e;
    }
  },

  async refreshPendingEdits() {
    try {
      const edits = await ipc.listPendingEdits();
      set({ pendingEdits: edits });
    } catch {
      set({ pendingEdits: [] });
    }
  },

  async applyEdit(id) {
    const edit = get().pendingEdits.find((e) => e.id === id);
    await ipc.applyPendingEdit(id);
    set((s) => ({ pendingEdits: s.pendingEdits.filter((e) => e.id !== id) }));
    await get().refreshGit();
    if (edit) await get().reloadOpenFile(edit.path);
  },

  async discardEdit(id) {
    await ipc.discardPendingEdit(id);
    set((s) => ({ pendingEdits: s.pendingEdits.filter((e) => e.id !== id) }));
  },

  async applyAllEdits() {
    await ipc.applyAllPendingEdits();
    const edited = get().pendingEdits.map((e) => e.path);
    set({ pendingEdits: [] });
    await get().refreshGit();
    for (const path of edited) await get().reloadOpenFile(path);
  },

  async loadSettings() {
    try {
      const settings = await ipc.getSettings();
      set({ settings });
    } catch (e) {
      set({ statusMessage: "Failed to load settings: " + String(e) });
    }
  },

  async saveSettings(settings) {
    await ipc.setSettings(settings);
    set({ settings, settingsOpen: false, statusMessage: "Settings saved" });
  },

  setSettingsOpen(open) {
    set({ settingsOpen: open });
  },

  async sendChat(prompt) {
    const root = get().projectRoot;
    if (!root || get().streaming) return;
    const history: ChatMessage[] = get()
      .chat.filter((m) => m.role === "user" || m.role === "assistant")
      .filter((m) => !m.streaming)
      .map((m) => ({ role: m.role, content: m.content }));

    set((s) => ({
      chat: [
        ...s.chat,
        { id: nextId("u"), role: "user", content: prompt },
        { id: nextId("a"), role: "assistant", content: "", streaming: true },
      ],
      streaming: true,
    }));

    try {
      const runId = await ipc.chat(history, prompt, root);
      set({ currentRunId: runId });
    } catch (e) {
      get().finishStreamingError(String(e));
    }
  },

  async stopChat() {
    const runId = get().currentRunId;
    if (runId) {
      try {
        await ipc.stopChat(runId);
      } catch {
        /* ignore */
      }
    }
    set({ streaming: false, currentRunId: null });
  },

  addPendingEdit(edit) {
    set((s) => ({
      pendingEdits: [...s.pendingEdits.filter((e) => e.id !== edit.id), edit],
    }));
  },

  appendStreamingDelta(content) {
    set((s) => {
      const chat = [...s.chat];
      const last = chat[chat.length - 1];
      if (last && last.streaming) {
        chat[chat.length - 1] = { ...last, content: last.content + content };
      }
      return { chat };
    });
  },

  appendToolCall(callId, name, args) {
    set((s) => ({
      chat: [
        ...s.chat,
        {
          id: nextId("t"),
          role: "tool",
          content: "",
          toolName: name,
          toolArgs: args,
          toolCallId: callId,
        },
      ],
    }));
  },

  appendToolResult(callId, name, result) {
    set((s) => ({
      chat: s.chat.map((m) =>
        m.toolCallId === callId
          ? { ...m, toolName: m.toolName ?? name, toolResult: result }
          : m
      ),
    }));
  },

  finishStreaming(message) {
    set((s) => {
      const chat = [...s.chat];
      const last = chat[chat.length - 1];
      if (last && last.streaming) {
        chat[chat.length - 1] = {
          ...last,
          content: message || last.content,
          streaming: false,
        };
      }
      return { chat, streaming: false, currentRunId: null };
    });
  },

  finishStreamingError(error) {
    set((s) => {
      const chat = [...s.chat];
      const last = chat[chat.length - 1];
      if (last && last.streaming) {
        chat[chat.length - 1] = {
          ...last,
          content: "Error: " + error,
          streaming: false,
        };
      }
      return { chat, streaming: false, currentRunId: null };
    });
    set({ statusMessage: "Agent error: " + error });
  },

  addTerminal(id) {
    set((s) => ({ terminalIds: [...s.terminalIds, id], activeTerminal: id }));
  },

  removeTerminal(id) {
    set((s) => {
      const terminalIds = s.terminalIds.filter((t) => t !== id);
      const activeTerminal =
        s.activeTerminal === id
          ? (terminalIds[terminalIds.length - 1] ?? null)
          : s.activeTerminal;
      return { terminalIds, activeTerminal };
    });
  },

  setActiveTerminal(id) {
    set({ activeTerminal: id });
  },

  setStatusMessage(message) {
    set({ statusMessage: message });
  },
}));
