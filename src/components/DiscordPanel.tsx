import { useEffect, useMemo, useRef, useState } from "react";
import { useDiscordStore } from "../state/discordStore";
import { useStore } from "../state/store";
import { ipc } from "../ipc/invoke";
import { pickTextFile, saveJsonFile } from "../ipc/dialog";
import PaneDivider from "./PaneDivider";
import ProvidersPanel from "./ProvidersPanel";
import DiscordTokenWizard from "./DiscordTokenWizard";
import GuildPicker from "./GuildPicker";
import ToolsPanel from "./ToolsPanel";
import { clampWidth, readStoredWidth } from "../lib/pane";
import { useHotkeys } from "../lib/hotkeys";
import { isTauri } from "../lib/tauri";
import { buildDiscussionContext } from "../lib/analysisContext";
import type {
  BotReport,
  DimensionSummary,
  GuardReport,
  PatternKind,
  ProfileReport,
} from "../ipc/contract";

function fmt(value: number, digits = 2): string {
  return value.toFixed(digits);
}

/** A Discord timestamp as a compact local `Mar 11, 09:12` label. */
function fmtTimestamp(raw: string | null): string {
  if (!raw) return "";
  const at = new Date(raw);
  if (Number.isNaN(at.getTime())) return raw;
  const day = at.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const time = at.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return `${day}, ${time}`;
}

/** A titled empty state, so a blank pane always says what to do next. */
function EmptyState({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="discord-empty-state">
      <strong>{title}</strong>
      <span>{hint}</span>
    </div>
  );
}

/** One- or two-letter initials for a display name, for the message avatar. */
function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

function polarityClass(polarity: string): string {
  if (polarity === "inverted-risk") return "risk";
  if (polarity === "desirable") return "good";
  return "";
}

const LEFT_MIN = 180;
const RIGHT_MIN = 240;
const CENTER_MIN = 220;
const LEFT_DEFAULT = 280;
const RIGHT_DEFAULT = 400;
/** Messages rendered at once; the rest load on demand. */
const MESSAGE_WINDOW = 150;

type RightTab = "analyze" | "flag" | "patterns" | "ask" | "tools";

const RIGHT_TABS: readonly (readonly [RightTab, string])[] = [
  ["analyze", "Analyze"],
  ["flag", "Flag"],
  ["patterns", "Patterns"],
  ["ask", "Ask DeepSeek"],
  ["tools", "Tools"],
];
/** A provider chip in the status bar: name, active model, and key/ready state. */
function ProviderChip({
  name,
  model,
  ready,
  readyLabel,
  missingLabel,
}: {
  name: string;
  model: string;
  ready: boolean;
  readyLabel: string;
  missingLabel: string;
}) {
  return (
    <div className="discord-provider" title={ready ? readyLabel : missingLabel}>
      <span className={`discord-dot ${ready ? "ok" : "warn"}`} />
      <span className="discord-provider-name">{name}</span>
      <span className="discord-provider-model">{model}</span>
    </div>
  );
}

export default function DiscordPanel() {
  const store = useDiscordStore();
  const [pasting, setPasting] = useState(false);
  const [pasteText, setPasteText] = useState("");
  const [patternForm, setPatternForm] = useState<{
    label: string;
    creator: string;
    category: string;
    kind: PatternKind;
    pattern: string;
  }>({ label: "", creator: "", category: "", kind: "substring", pattern: "" });
  const [flagNote, setFlagNote] = useState("");
  const [flagAction, setFlagAction] = useState("review");
  const [discussionPrompt, setDiscussionPrompt] = useState("");
  const [dceFormat, setDceFormat] = useState("Json");
  const [leftWidth, setLeftWidth] = useState(() =>
    readStoredWidth("orbit.discord.leftWidth", LEFT_DEFAULT)
  );
  const [rightWidth, setRightWidth] = useState(() =>
    readStoredWidth("orbit.discord.rightWidth", RIGHT_DEFAULT)
  );
  const [providersOpen, setProvidersOpen] = useState(false);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [peopleQuery, setPeopleQuery] = useState("");
  const [showAllMembers, setShowAllMembers] = useState(false);
  const [messageWindow, setMessageWindow] = useState(MESSAGE_WINDOW);
  const [rightTab, setRightTab] = useState<RightTab>("analyze");
  const [notice, setNotice] = useState<string | null>(null);
  const filterRef = useRef<HTMLInputElement | null>(null);
  const leftBase = useRef(LEFT_DEFAULT);
  const rightBase = useRef(RIGHT_DEFAULT);
  const settings = useStore((s) => s.settings);

  useEffect(() => {
    void store.refreshDceStatus();
    void store.restoreDataset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Persist the pane widths so the layout is stable across restarts.
  useEffect(() => {
    window.localStorage.setItem("orbit.discord.leftWidth", String(leftWidth));
  }, [leftWidth]);
  useEffect(() => {
    window.localStorage.setItem("orbit.discord.rightWidth", String(rightWidth));
  }, [rightWidth]);

  // Keep the panes inside the window when it is resized.
  useEffect(() => {
    function onResize() {
      const total = window.innerWidth;
      setLeftWidth((w) => clampWidth(w, LEFT_MIN, total - RIGHT_MIN - CENTER_MIN));
      setRightWidth((w) => clampWidth(w, RIGHT_MIN, total - LEFT_MIN - CENTER_MIN));
    }
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // A secret the bridge withheld still counts as configured.
  const tsReady = Boolean(settings?.typesafe_api_key) || Boolean(settings?.typesafe_api_key_set);
  const dsReady = Boolean(settings?.api_key) || Boolean(settings?.api_key_set);
  const dcReady =
    Boolean(settings?.discord_token) ||
    Boolean(settings?.discord_token_set) ||
    Boolean(store.account);
  const liveChannelId = store.selectedLiveChannelId;
  const liveChannelName = store.selectedLiveChannelName;

  async function pickFile() {
    const picked = await pickTextFile();
    if (picked) {
      await store.importText(picked.name, picked.content);
    }
  }

  async function submitPaste() {
    if (pasteText.trim()) {
      await store.importText("Pasted log", pasteText);
      setPasting(false);
      setPasteText("");
    }
  }

  async function submitPattern() {
    try {
      await store.flagPattern({
        label: patternForm.label,
        creator: patternForm.creator,
        category: patternForm.category,
        kind: patternForm.kind,
        pattern: patternForm.pattern,
      });
      setPatternForm({ label: "", creator: "", category: "", kind: "substring", pattern: "" });
    } catch {
      /* error is surfaced in the store */
    }
  }

  const user = store.dataset?.users.find((u) => u.id === store.selectedUserId) ?? null;
  const userFlag = store.flags.find(
    (f) => f.entity_type === "user" && f.entity_id === store.selectedUserId
  );
  // A report describes the user it was run for. If that is no longer the
  // selected user, say so rather than interpreting someone else's numbers.
  const analysisSubjectId = store.analysis?.subjectId ?? null;
  const analysisSubjectName =
    store.dataset?.users.find((u) => u.id === analysisSubjectId)?.name ?? null;
  const subjectMismatch = analysisSubjectId !== null && analysisSubjectId !== store.selectedUserId;

  // One map for the whole render: resolving authors per message was O(n*m),
  // which is a million comparisons for a 1000-message, 1000-member dataset.
  const usersById = useMemo(
    () => new Map((store.dataset?.users ?? []).map((u) => [u.id, u])),
    [store.dataset]
  );
  const flaggedIds = useMemo(
    () => new Set(store.flags.filter((f) => f.entity_type === "user").map((f) => f.entity_id)),
    [store.flags]
  );
  const visibleMessages = useMemo(() => {
    const needle = store.query.trim().toLowerCase();
    if (!needle) return store.messages;
    return store.messages.filter((m) => m.content.toLowerCase().includes(needle));
  }, [store.messages, store.query]);
  // Render a window rather than every match: a fetched channel can hold a
  // thousand messages, and each row is a bordered card with an avatar.
  const shownMessages = useMemo(
    () => visibleMessages.slice(0, messageWindow),
    [visibleMessages, messageWindow]
  );

  // People who actually posted lead the list; the full roster is opt-in, since
  // a fetched guild can contribute a thousand members with no messages.
  const roster = store.dataset?.users ?? [];
  const people = useMemo(() => {
    const needle = peopleQuery.trim().toLowerCase();
    const base = showAllMembers ? roster : roster.filter((u) => u.message_count > 0);
    const filtered = needle ? base.filter((u) => u.name.toLowerCase().includes(needle)) : base;
    return [...filtered].sort((a, b) => b.message_count - a.message_count);
  }, [roster, showAllMembers, peopleQuery]);
  const silentMembers = roster.length - roster.filter((u) => u.message_count > 0).length;

  // Each control reads only its own operation, so a long export no longer
  // disables screening or paints "Working…" over an unrelated tab.
  const analysisBusy = store.busy.includes("analysis");
  const screeningBusy = store.busy.includes("screening");
  const guildBusy = store.busy.includes("guild");
  const channelBusy = store.busy.includes("channel");
  const exportBusy = store.busy.includes("export");

  async function askDeepSeek() {
    const ctx = buildDiscussionContext(store.analysis);
    if (!ctx) return;
    const prompt = discussionPrompt.trim() || "Summarize the key findings and what a human reviewer should check.";
    await store.discuss(prompt, ctx);
  }

  /** Save the current dataset as a DiscordChatExporter-compatible JSON file. */
  async function exportJson() {
    if (!store.dataset) {
      setNotice("Nothing to export yet — fetch or import a channel first.");
      return;
    }
    const stem = store.dataset.guild.name.replace(/[^\w.-]+/g, "_") || "discord";
    try {
      const value = await ipc.discordExportJsonText();
      const written = await saveJsonFile(`${stem}.json`, value);
      if (written) setNotice(`Saved ${written}`);
    } catch (e) {
      setNotice(`Export failed: ${String(e)}`);
    }
  }

  /** Open Discord Web in an isolated in-app browser window (or a browser tab). */
  function openDiscordWeb() {
    if (isTauri()) {
      void ipc.openDiscordWeb();
    } else {
      window.open("https://discord.com/app", "_blank", "noopener,noreferrer");
    }
  }

  useHotkeys({
    "mod+1": () => setRightTab("analyze"),
    "mod+2": () => setRightTab("flag"),
    "mod+3": () => setRightTab("patterns"),
    "mod+4": () => setRightTab("ask"),
    "mod+5": () => setRightTab("tools"),
    "mod+k": () => filterRef.current?.focus(),
    "mod+e": () => void exportJson(),
    "mod+enter": () => {
      if (store.selectedUserId) void store.runAnalysis("guard", store.selectedUserId);
    },
  });

  return (
    <div className="discord-workspace">
      <div className="discord-statusbar">
        <ProviderChip
          name="Jev (TypeSafe)"
          model={settings?.typesafe_model ?? "jev-latest"}
          ready={tsReady}
          readyLabel="TypeSafe API key configured"
          missingLabel="No TypeSafe API key — enable Dry run, or set one in Settings"
        />
        <ProviderChip
          name="DeepSeek"
          model={settings?.model ?? "deepseek-chat"}
          ready={dsReady}
          readyLabel="DeepSeek API key configured"
          missingLabel="No DeepSeek API key — set one in Settings to discuss analyses"
        />
        <ProviderChip
          name="Discord"
          model={store.account ? `@${store.account.username}` : "not connected"}
          ready={dcReady}
          readyLabel="Discord token configured"
          missingLabel="No Discord token — set one in Settings for live browsing"
        />
        <div className="discord-status-spacer" />
        <button
          className={"discord-status-btn" + (providersOpen ? " active" : "")}
          onClick={() => setProvidersOpen((p) => !p)}
          title="Configure Jev (TypeSafe), DeepSeek, and Discord"
        >
          {providersOpen ? "▾" : "▸"} ⚙ Providers &amp; keys
        </button>
      </div>

      {providersOpen && <ProvidersPanel />}
      {wizardOpen && <DiscordTokenWizard onClose={() => setWizardOpen(false)} />}

      <div className="discord-panel">
        <div className="discord-col discord-left" style={{ width: leftWidth }}>
          <div className="discord-live">
          <div className="discord-section-title">Live Discord</div>
          {!store.account ? (
            <>
              <button className="primary" onClick={() => void store.connectDiscord()}>
                Connect Discord
              </button>
              <button onClick={() => setWizardOpen((w) => !w)}>
                {wizardOpen ? "Hide setup guide" : "Set up token…"}
              </button>
              {!dcReady && (
                <div className="discord-hint">
                  No token yet — the setup guide shows where to copy one from Discord Web.
                </div>
              )}
            </>
          ) : (
            <div className="discord-account">
              <strong>{store.account.global_name ?? store.account.username}</strong>
              <span>@{store.account.username}</span>
            </div>
          )}

          <button
            onClick={openDiscordWeb}
            title="Sign in to Discord Web in an isolated in-app browser window. The app never reads that page or its session."
          >
            Open Discord Web
          </button>

          {store.account && store.guilds.length > 0 && (
            <>
              <div className="discord-section-title">Server</div>
              <GuildPicker
                guilds={store.guilds}
                activeId={store.selectedGuildId}
                onSelect={(id) => void store.selectGuild(id)}
              />
              {guildBusy && <span className="discord-hint">Loading channels…</span>}
            </>
          )}

          {store.selectedGuildId && store.channels.length > 0 && (
            <>
              <div className="discord-section-title">
                Channels ({store.channels.length})
              </div>
              <ul className="discord-list">
                {store.channels.map((c) => (
                  <li
                    key={c.id}
                    className={liveChannelId === c.id ? "active" : ""}
                    onClick={() => store.selectLiveChannel(c.id, c.name)}
                  >
                    # {c.name}
                  </li>
                ))}
              </ul>
              {liveChannelId && (
                <button
                  className="primary"
                  onClick={() => void store.fetchChannel()}
                  disabled={channelBusy}
                >
                  {channelBusy ? "Fetching…" : `Fetch #${liveChannelName}`}
                </button>
              )}
            </>
          )}

          {liveChannelId && (
            <div className="discord-dce">
              <div className="discord-section-title">DiscordChatExporter</div>
              <div className="discord-flag-row">
                <select value={dceFormat} onChange={(e) => setDceFormat(e.target.value)}>
                  <option>Json</option>
                  <option>HtmlDark</option>
                  <option>HtmlLight</option>
                  <option>PlainText</option>
                  <option>Csv</option>
                </select>
                <button
                  onClick={() => void store.dceExport(liveChannelId, dceFormat)}
                  disabled={exportBusy}
                >
                  {exportBusy ? "Exporting…" : `Export #${liveChannelName}`}
                </button>
              </div>
              <div className="discord-dce-status">
                {store.dceStatus?.installed
                  ? "DCE ready"
                  : "DCE downloads on first export"}
              </div>
            </div>
          )}
        </div>

        <div className="discord-block">
          <div className="discord-section-title">Import &amp; export</div>
          <div className="discord-import">
            <button onClick={() => void pickFile()}>Import export…</button>
            <button onClick={() => setPasting((p) => !p)}>Paste log</button>
            <button
              onClick={() => void exportJson()}
              disabled={!store.dataset}
              title="Save the current view as DiscordChatExporter-compatible JSON (⌘E)"
            >
              Export JSON
            </button>
            {notice && <div className="discord-notice">{notice}</div>}
            {pasting && (
              <div className="discord-paste">
                <textarea
                  value={pasteText}
                  onChange={(e) => setPasteText(e.target.value)}
                  placeholder="Paste a DiscordChatExporter plaintext export or copied chat…"
                  rows={8}
                />
                <button className="primary" onClick={() => void submitPaste()}>
                  Import pasted
                </button>
              </div>
            )}
          </div>
        </div>

        {store.dataset && (
          <div className="discord-block">
            <div className="discord-guild">
              <strong>{store.dataset.guild.name}</strong>
              <span>
                {store.dataset.message_count} messages · {store.dataset.users.length} users
              </span>
            </div>

            <div className="discord-section-title">Channels</div>
            <ul className="discord-list">
              <li
                className={store.selectedChannelId === null ? "active" : ""}
                onClick={() => store.selectChannel(null)}
              >
                All channels
              </li>
              {store.dataset.channels.map((c) => (
                <li
                  key={c.id ?? c.name}
                  className={store.selectedChannelId === c.id ? "active" : ""}
                  onClick={() => store.selectChannel(c.id)}
                >
                  {c.name}
                </li>
              ))}
            </ul>

            <div className="discord-section-title">
              People ({people.length}
              {showAllMembers ? ` of ${roster.length}` : ""})
              {store.selectedUserIds.length > 1 && (
                <span className="discord-hint"> · {store.selectedUserIds.length} selected</span>
              )}
            </div>
            <input
              className="discord-search"
              placeholder="Filter people…"
              value={peopleQuery}
              onChange={(e) => setPeopleQuery(e.target.value)}
            />
            {silentMembers > 0 && (
              <button
                className="discord-clear"
                onClick={() => setShowAllMembers((v) => !v)}
                title="Members who never posted in this window"
              >
                {showAllMembers
                  ? "Show only people who posted"
                  : `Show ${silentMembers} members with no messages`}
              </button>
            )}
            <ul className="discord-list discord-users discord-scroll">
              {people.map((u) => {
                const flagged = flaggedIds.has(u.id);
                return (
                  <li
                    key={u.id}
                    className={store.selectedUserIds.includes(u.id) ? "active" : ""}
                    title="Click to select. ⌘-click to add to the selection."
                    onClick={(e) => store.selectUser(u.id, e.metaKey || e.ctrlKey)}
                  >
                    <span className="discord-user-name">
                      {u.name}
                      {u.is_bot && <span className="bot-badge">BOT</span>}
                      {flagged && <span className="flag-badge">⚑</span>}
                    </span>
                    <span className="discord-user-count">
                      {u.message_count > 0 ? u.message_count : "—"}
                    </span>
                  </li>
                );
              })}
              {people.length === 0 && (
                <li className="discord-hint">
                  {roster.length === 0 ? "No users in this dataset." : "No person matches that filter."}
                </li>
              )}
            </ul>
          </div>
        )}
      </div>

      <PaneDivider
        onStart={() => {
          leftBase.current = leftWidth;
        }}
        onDelta={(delta) =>
          setLeftWidth(
            clampWidth(leftBase.current + delta, LEFT_MIN, window.innerWidth - rightWidth - CENTER_MIN)
          )
        }
        onReset={() => setLeftWidth(LEFT_DEFAULT)}
      />

      <div className="discord-col discord-center">
        <div className="discord-messages-header">
          <input
            ref={filterRef}
            className="discord-search"
            placeholder="Filter messages…  (⌘K)"
            value={store.query}
            onChange={(e) => store.setQuery(e.target.value)}
          />
          <span>
            {visibleMessages.length} shown
            {store.messages.length !== visibleMessages.length &&
              ` of ${store.messages.length}`}
            {store.selectedMessages.length > 0 && ` · ${store.selectedMessages.length} selected`}
          </span>
          {store.selectedMessages.length > 0 && (
            <button className="discord-clear" onClick={() => store.clearMessages()}>
              Clear
            </button>
          )}
        </div>
        <div className="discord-messages">
          {shownMessages.map((m) => {
            const author = usersById.get(m.author_id);
            const name = author?.name ?? m.author_id;
            // Selection holds message text: it is what gets screened.
            const selected = store.selectedMessages.includes(m.content);
            return (
              <div
                key={m.id ?? m.timestamp ?? m.content}
                className={"discord-message" + (selected ? " selected" : "")}
                title="Click to select. ⌘-click to add to the selection."
                onClick={(e) => store.selectMessage(m.content, e.metaKey || e.ctrlKey)}
              >
                <div className={"avatar" + (author?.is_bot ? " avatar-bot" : "")}>
                  {initials(name)}
                </div>
                <div className="discord-message-main">
                  <div className="discord-message-meta">
                    <strong>{name}</strong>
                    {author?.is_bot && <span className="bot-badge">BOT</span>}
                    <span className="discord-timestamp">{fmtTimestamp(m.timestamp)}</span>
                  </div>
                  <div className="discord-message-body">{m.content}</div>
                </div>
              </div>
            );
          })}
          {shownMessages.length < visibleMessages.length && (
            <button
              className="discord-more"
              onClick={() => setMessageWindow((n) => n + MESSAGE_WINDOW)}
            >
              Show {Math.min(MESSAGE_WINDOW, visibleMessages.length - shownMessages.length)} more
              ({visibleMessages.length - shownMessages.length} remaining)
            </button>
          )}
          {visibleMessages.length === 0 && (
            <EmptyState
              title={store.dataset ? "No matching messages" : "No messages yet"}
              hint={
                store.messages.length > 0
                  ? "Clear the filter or pick another user."
                  : store.dataset
                    ? "Pick another user or channel, or clear the filter."
                    : "Fetch a channel under Live Discord, or import an export."
              }
            />
          )}
        </div>
      </div>

      <PaneDivider
        onStart={() => {
          rightBase.current = rightWidth;
        }}
        onDelta={(delta) =>
          setRightWidth(
            clampWidth(rightBase.current - delta, RIGHT_MIN, window.innerWidth - leftWidth - CENTER_MIN)
          )
        }
        onReset={() => setRightWidth(RIGHT_DEFAULT)}
      />

      <div className="discord-col discord-right" style={{ width: rightWidth }}>
        <div className="discord-tabs">
          {RIGHT_TABS.map(([id, label]) => (
            <button
              key={id}
              className={"discord-tab" + (rightTab === id ? " active" : "")}
              onClick={() => setRightTab(id)}
            >
              {label}
            </button>
          ))}
        </div>

        {store.error && <div className="discord-error">{store.error}</div>}
        {analysisBusy && <div className="discord-loading">Analysing…</div>}

        {rightTab === "analyze" && (
          <>
            <div className="discord-section-title">
              Jev analysis · {settings?.typesafe_model ?? "jev-latest"}
            </div>
            {user ? (
              <>
                <button
                  className="primary"
                  onClick={() => void store.runFullAnalysis(user.id)}
                  disabled={analysisBusy}
                  title="Run the profile, bot-detection, and guardrails assessments together"
                >
                  {analysisBusy ? "Analysing…" : "Run full analysis"}
                </button>
                <div className="discord-actions">
                  <button
                    onClick={() => void store.runAnalysis("profile", user.id)}
                    disabled={analysisBusy}
                  >
                    Psychological profile
                  </button>
                  <button
                    onClick={() => void store.runAnalysis("bot", user.id)}
                    disabled={analysisBusy}
                  >
                    Bot detection
                  </button>
                  <button
                    onClick={() => void store.runAnalysis("guard", user.id)}
                    disabled={analysisBusy}
                  >
                    Guardrails route
                  </button>
                </div>
              </>
            ) : null}
            <div className="discord-options">
              <label className="discord-check">
                <input
                  type="checkbox"
                  checked={store.dryRun}
                  onChange={(e) => store.setDryRun(e.target.checked)}
                />
                Dry run (no API key)
              </label>
              <label className="discord-check">
                Policy
                <select
                  value={store.guardPolicy}
                  onChange={(e) => store.setGuardPolicy(e.target.value)}
                >
                  {store.guardPolicies.map((p) => (
                    <option key={p} value={p}>
                      {p}
                    </option>
                  ))}
                </select>
              </label>
              <label
                className="discord-check"
                title="Have DeepSeek write the interpretation automatically after each analysis"
              >
                <input
                  type="checkbox"
                  checked={store.autoNarrative}
                  onChange={(e) => store.setAutoNarrative(e.target.checked)}
                />
                DeepSeek narrative
              </label>
            </div>
            {!user && (
              <EmptyState
                title="Nothing selected"
                hint={
                  store.dataset
                    ? "Pick a user in the left column, then run an analysis."
                    : "Fetch a channel under Live Discord, or import an export, to get started."
                }
              />
            )}
            {store.generatedProbes !== null && (
              <div className="discord-discuss">
                <strong>DeepSeek probes</strong>
                {store.discussionStreaming && !store.generatedProbes ? (
                  <span className="discord-hint">Writing…</span>
                ) : (
                  <div className="discord-discussion">{store.generatedProbes}</div>
                )}
              </div>
            )}
            {(store.discussion ||
              (store.discussionStreaming && store.discussionTarget === "chat")) && (
              <div className="discord-discuss">
                <strong>DeepSeek narrative</strong>
                {store.discussion ? (
                  <div className="discord-discussion">{store.discussion}</div>
                ) : (
                  <span className="discord-hint">Writing…</span>
                )}
              </div>
            )}
            <AnalysisDetail />
          </>
        )}

        {rightTab === "flag" && (
          <>
            {store.selectedMessages.length > 0 ? (
              <div className="discord-screen">
                <strong>
                  {store.selectedMessages.length === 1
                    ? "Selected message"
                    : `${store.selectedMessages.length} selected messages`}
                </strong>
                {store.selectedMessages.map((text, i) => (
                  <div key={i} className="discord-screen-text">
                    {text}
                  </div>
                ))}
                <div className="discord-actions">
                  <button
                    onClick={() =>
                      void store.screenMessage(store.selectedMessages.join("\n\n"), "input")
                    }
                  >
                    Screen as input
                  </button>
                  <button
                    onClick={() =>
                      void store.screenMessage(store.selectedMessages.join("\n\n"), "output")
                    }
                  >
                    Screen as output
                  </button>
                  {screeningBusy && <span className="discord-hint">Screening…</span>}
                </div>
              </div>
            ) : (
              <EmptyState
                title="No message selected"
                hint="Click a message in the middle pane to screen it. ⌘-click to screen several together."
              />
            )}

            {store.screening && (
              <>
                {store.dryRun && (
                  <div className="discord-warn">
                    Dry run: these hazard values are synthetic, not a TypeSafe assessment. Turn
                    off “Dry run” in the Analyze tab to screen for real.
                  </div>
                )}
                <GuardView
                  report={store.screening}
                  title={`Message screening · ${store.screening.source}`}
                />
              </>
            )}

            {user ? (
              <div className="discord-flag">
                <strong>
                  Flag {user.name}
                  {store.selectedUserIds.length > 1 && ` + ${store.selectedUserIds.length - 1} more`}
                </strong>
                <div className="discord-flag-row">
                  <select value={flagAction} onChange={(e) => setFlagAction(e.target.value)}>
                    <option value="review">review</option>
                    <option value="block">block</option>
                    <option value="support">support</option>
                    <option value="flagged">flagged</option>
                  </select>
                  <input
                    placeholder="Note"
                    value={flagNote}
                    onChange={(e) => setFlagNote(e.target.value)}
                  />
                  <button
                    onClick={() =>
                      void store.setFlag("user", user.id, user.name, flagAction, flagNote)
                    }
                  >
                    {userFlag ? "Update flag" : "Flag"}
                  </button>
                  {store.selectedUserIds.length > 1 && (
                    <button
                      title="Apply the same action and note to every selected user"
                      onClick={() =>
                        void Promise.all(
                          store.selectedUserIds.map((id) => {
                            const name =
                              store.dataset?.users.find((u) => u.id === id)?.name ?? id;
                            return store.setFlag("user", id, name, flagAction, flagNote);
                          })
                        )
                      }
                    >
                      Flag all {store.selectedUserIds.length}
                    </button>
                  )}
                </div>
                {userFlag && (
                  <button
                    className="discord-clear"
                    onClick={() => void store.clearFlag("user", user.id)}
                  >
                    Clear flag (currently {userFlag.action})
                  </button>
                )}
              </div>
            ) : (
              <EmptyState title="No user selected" hint="Select a user to flag them." />
            )}

            <div className="discord-flags-list">
              <strong>Saved flags ({store.flags.length})</strong>
              {store.flags.length === 0 ? (
                <span className="discord-hint">No flags yet.</span>
              ) : (
                <ul className="discord-list">
                  {store.flags.map((f) => (
                    <li key={`${f.entity_type}:${f.entity_id}`}>
                      <span className="discord-user-name">
                        {f.label}
                        <em className={`flag-action flag-${f.action}`}>{f.action}</em>
                      </span>
                      <button
                        onClick={() => void store.clearFlag(f.entity_type, f.entity_id)}
                        title="Clear flag"
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </>
        )}

        {rightTab === "patterns" && (
          <div className="discord-patterns">
            <strong>Flagged patterns</strong>
            {store.patterns.length === 0 ? (
              <span className="discord-hint">
                No patterns yet. Add a reusable bot signature below.
              </span>
            ) : (
              <ul className="discord-list">
                {store.patterns.map((p) => (
                  <li key={p.id} className="discord-pattern">
                    <span>
                      {p.label} <em>{p.kind}</em>
                    </span>
                    <button onClick={() => void store.removePattern(p.id)}>×</button>
                  </li>
                ))}
              </ul>
            )}
            <div className="discord-pattern-form">
              <input
                placeholder="Label"
                value={patternForm.label}
                onChange={(e) => setPatternForm({ ...patternForm, label: e.target.value })}
              />
              <input
                placeholder="Creator"
                value={patternForm.creator}
                onChange={(e) => setPatternForm({ ...patternForm, creator: e.target.value })}
              />
              <input
                placeholder="Category"
                value={patternForm.category}
                onChange={(e) => setPatternForm({ ...patternForm, category: e.target.value })}
              />
              <select
                value={patternForm.kind}
                onChange={(e) =>
                  setPatternForm({ ...patternForm, kind: e.target.value as PatternKind })
                }
              >
                <option value="substring">substring</option>
                <option value="word">word</option>
                <option value="regex">regex</option>
              </select>
              <input
                placeholder="Pattern text"
                value={patternForm.pattern}
                onChange={(e) => setPatternForm({ ...patternForm, pattern: e.target.value })}
              />
              <button onClick={() => void submitPattern()}>Add pattern</button>
            </div>
            <button onClick={() => void store.matchPatterns(store.selectedUserId)}>
              Match patterns against this view
            </button>
            {store.patternReport && <PatternReportView report={store.patternReport} />}
          </div>
        )}

        {rightTab === "ask" && (
          <div className="discord-discuss">
            <strong>Ask DeepSeek ({settings?.model ?? "deepseek-chat"}) about this analysis</strong>
            {subjectMismatch && (
              <div className="discord-warn">
                The report on screen is for {analysisSubjectName ?? "another user"}. Run an
                analysis for {user?.name ?? "this user"} before interpreting it.
              </div>
            )}
            <div className="discord-actions">
              <button
                disabled={!user || analysisBusy}
                onClick={() => void store.generateProbes(user!.id)}
                title="Have DeepSeek write follow-up probes from a fresh bot report"
              >
                Generate probes
              </button>
              <button
                disabled={!store.analysis || subjectMismatch}
                onClick={() =>
                  void store.discuss(
                    "Explain this result as if to a non-specialist reviewer.",
                    buildDiscussionContext(store.analysis)
                  )
                }
              >
                Explain simply
              </button>
              <button
                disabled={!store.analysis || subjectMismatch}
                onClick={() =>
                  void store.discuss(
                    "Draft a short review note for a human moderator: what to check and why, " +
                      "with the caveats that matter. Do not make a decision about the person.",
                    buildDiscussionContext(store.analysis)
                  )
                }
              >
                Draft review note
              </button>
            </div>
            <input
              placeholder="Ask about the selected user's report…"
              value={discussionPrompt}
              onChange={(e) => setDiscussionPrompt(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void askDeepSeek();
              }}
            />
            <button onClick={() => void askDeepSeek()} disabled={store.discussionStreaming}>
              {store.discussionStreaming ? "Thinking…" : "Discuss"}
            </button>
            {store.generatedProbes !== null ? (
              <div className="discord-discussion">
                <strong>DeepSeek probes</strong>
                {store.generatedProbes || (store.discussionStreaming ? "Writing…" : "")}
              </div>
            ) : store.discussion ? (
              <div className="discord-discussion">{store.discussion}</div>
            ) : (
              <span className="discord-hint">
                {store.analysis
                  ? "Ask a question, or use a preset above."
                  : "Run an analysis, then ask DeepSeek to interpret it."}
              </span>
            )}
          </div>
        )}

        {rightTab === "tools" && <ToolsPanel />}
        </div>
      </div>
    </div>
  );
}

function AnalysisDetail() {
  const analysis = useDiscordStore((s) => s.analysis);
  const dataset = useDiscordStore((s) => s.dataset);
  if (!analysis) return null;
  const { profile, bot, guard } = analysis;
  // The subject comes from the report itself, never from the live selection,
  // so a report can never be relabelled with another user's name.
  const subject = dataset?.users.find((u) => u.id === analysis.subjectId)?.name ?? null;
  // A full analysis carries all three reports; a single run carries one.
  if (profile && bot && guard) {
    return (
      <>
        <ProfileView report={profile} />
        <BotView report={bot} subject={subject} />
        <GuardView report={guard} />
      </>
    );
  }
  if (profile) return <ProfileView report={profile} />;
  if (bot) return <BotView report={bot} subject={subject} />;
  if (guard) return <GuardView report={guard} />;
  return null;
}

/** A compact 0–1 score bar with its numeric value. */
function ScoreBar({ value, tone = "" }: { value: number; tone?: string }) {
  const pct = Math.round(Math.max(0, Math.min(1, value)) * 100);
  return (
    <div className="score-bar" title={value.toFixed(3)}>
      <div className="score-bar-track">
        <div className={`score-bar-fill ${tone}`} style={{ width: `${pct}%` }} />
      </div>
      <span className="score-bar-value">{value.toFixed(2)}</span>
    </div>
  );
}

function ProfileView({ report }: { report: ProfileReport }) {
  const flagged = report.dimensions.filter(
    (d) => d.polarity === "inverted-risk" && d.score >= 0.6
  );
  return (
    <div className="discord-report">
      <h3>Profile: {report.author ?? report.source}</h3>
      <p>
        {report.scored_count}/{report.message_count} messages ·{" "}
        {report.dimensions.length} dimensions
      </p>
      {flagged.length > 0 && (
        <div className="discord-warn">
          Elevated inverted-risk: {flagged.map((d) => d.label).join(", ")}
        </div>
      )}
      <div className="discord-table-wrap">
        <table className="discord-table">
          <thead>
            <tr>
              <th>Dimension</th>
              <th>Score</th>
              <th>n</th>
              <th>Conf</th>
            </tr>
          </thead>
          <tbody>
            {report.dimensions.map((d) => (
              <tr key={d.id}>
                <td className={polarityClass(d.polarity)} title={d.direction}>
                  {d.label}
                </td>
                <td>
                  <ScoreBar value={d.score} tone={polarityClass(d.polarity)} />
                </td>
                <td>{d.n}</td>
                <td>{fmt(d.confidence)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <EvidenceList dimensions={report.dimensions} />
    </div>
  );
}

function BotView({ report, subject }: { report: BotReport; subject: string | null }) {
  return (
    <div className="discord-report">
      <h3>Bot detection: {subject ?? report.source}</h3>
      <div className={`verdict-badge verdict-${report.verdict}`}>{report.verdict}</div>
      <p className="discord-caveat">composite machine likelihood {fmt(report.likelihood)}</p>
      <div className="discord-table-wrap">
        <table className="discord-table">
          <thead>
            <tr>
              <th>Dimension</th>
              <th>Score</th>
              <th>Weight</th>
              <th>n</th>
            </tr>
          </thead>
          <tbody>
            {report.dimensions.map((d) => (
              <tr key={d.id}>
                <td title={d.direction}>{d.label}</td>
                <td>
                  <ScoreBar value={d.score} tone={d.score >= 0.6 ? "risk" : ""} />
                </td>
                <td>{fmt(d.weight)}</td>
                <td>{d.n}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <details>
        <summary>Interview probes</summary>
        {report.probes.map((p) => (
          <div key={p.dimension} className="discord-probes">
            <strong>{p.label}</strong>
            <ul>
              {p.items.map((item, i) => (
                <li key={i}>{item}</li>
              ))}
            </ul>
          </div>
        ))}
      </details>
    </div>
  );
}

function GuardView({ report, title = "Guardrails" }: { report: GuardReport; title?: string }) {
  return (
    <div className="discord-report">
      <h3>{title}</h3>
      <div className={`action-badge action-${report.action}`}>{report.action.toUpperCase()}</div>
      <p>Policy {report.policy} · severity {fmt(report.severity)}</p>
      <div className="discord-table-wrap">
        <table className="discord-table">
          <thead>
            <tr>
              <th>Hazard</th>
              <th>Value</th>
              <th>Triggered</th>
            </tr>
          </thead>
          <tbody>
            {report.hazards.map((h) => (
              <tr key={h.id} className={h.triggered ? "risk" : ""}>
                <td>{h.label}</td>
                <td>{fmt(h.value)}</td>
                <td>{h.triggered ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {report.flagged.length > 0 && (
        <div className="discord-warn">Flagged: {report.flagged.join(", ")}</div>
      )}
      <p className="discord-caveat">
        Heuristic routing, not a decision about the person. self_harm routes to support, never a block.
      </p>
    </div>
  );
}

function EvidenceList({ dimensions }: { dimensions: DimensionSummary[] }) {
  const withEvidence = dimensions.filter((d) => d.evidence.length > 0);
  if (withEvidence.length === 0) return null;
  return (
    <details>
      <summary>Evidence excerpts</summary>
      {withEvidence.map((d) => (
        <div key={d.id} className="discord-evidence">
          <strong>{d.label}</strong>
          {d.evidence.map((e) => (
            <div key={e.index}>#{e.index}: “{e.excerpt}”</div>
          ))}
        </div>
      ))}
    </details>
  );
}

function PatternReportView({ report }: { report: import("../ipc/contract").PatternMatchReport }) {
  return (
    <div className="discord-report">
      <strong>Pattern match: {report.matchedPatterns}/{report.patternsScanned} patterns</strong>
      <ul>
        {report.patterns.map((p) => (
          <li key={p.patternId}>
            {p.label}: {p.matches} match(es)
          </li>
        ))}
      </ul>
      {report.gapStats && (
        <p>Gap CV {fmt(report.gapStats.coefficientOfVariation, 3)}</p>
      )}
    </div>
  );
}
