import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import ChatPanel from "./ChatPanel";
import { ipc } from "../ipc/invoke";
import { isTauri } from "../lib/tauri";
import { useStore } from "../state/store";
import {
  CHAIN_EVENT,
  DSH_EVENT,
  type ChainEvent,
  type ChainRun,
  type DshEvent,
  type DshStatus,
  type WorkspaceMode,
} from "../ipc/contract";

type Tab = "chat" | "chain" | "harness";

const MAX_HARNESS_LINES = 400;

/** A short label for a chain's state. */
function statusLabel(run: ChainRun): string {
  switch (run.status) {
    case "passed":
      return "passed";
    case "failed":
      return "failed";
    case "cancelled":
      return "cancelled";
    case "blocked":
      return "blocked";
    default:
      return `cycle ${run.cycle}/${run.max_cycles}`;
  }
}

/**
 * The Chained tab: hand the agent a goal, let it edit, then verify with the
 * project's own commands, repeating until the check passes or the budget runs
 * out. Verification output is fed back into the next cycle automatically.
 */
function ChainPanel({ workspace }: { workspace: string }) {
  const [mode, setMode] = useState<WorkspaceMode | null>(null);
  const [runs, setRuns] = useState<ChainRun[]>([]);
  const [lines, setLines] = useState<string[]>([]);
  const [goal, setGoal] = useState("");
  const [maxCycles, setMaxCycles] = useState(3);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    ipc
      .workspaceMode(workspace)
      .then((result) => {
        if (!cancelled) setMode(result);
      })
      .catch(() => {
        if (!cancelled) setMode(null);
      });
    return () => {
      cancelled = true;
    };
  }, [workspace]);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<ChainEvent>(CHAIN_EVENT, (event) => {
      const e = event.payload;
      switch (e.type) {
        case "log":
          setLines((prev) => [...prev, e.line].slice(-MAX_HARNESS_LINES));
          break;
        case "verify":
          setLines((prev) =>
            [...prev, `verify (cycle ${e.cycle}): ${e.ok ? "PASS" : "FAIL"} — ${e.detail}`].slice(
              -MAX_HARNESS_LINES
            )
          );
          break;
        case "done":
          setLines((prev) => [...prev, `chain ${e.status}: ${e.reason}`].slice(-MAX_HARNESS_LINES));
          setBusy(false);
          void ipc.getChain(null).then(setRuns).catch(() => undefined);
          break;
        case "cycle_start":
        case "cycle":
          void ipc.getChain(null).then(setRuns).catch(() => undefined);
          break;
      }
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const running = runs.find((run) => run.status === "running");

  async function start() {
    const text = goal.trim();
    if (!text) return;
    setError(null);
    setLines([]);
    setBusy(true);
    try {
      await ipc.startChain(text, workspace, maxCycles);
      setGoal("");
      setRuns(await ipc.getChain(null));
    } catch (e) {
      setBusy(false);
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function stop() {
    if (!running) return;
    try {
      await ipc.stopChain(running.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const selfHosting = mode?.self_hosting ?? false;

  return (
    <div className="chain-panel">
      <div className={selfHosting ? "selfhost-badge on" : "selfhost-badge"}>
        {selfHosting
          ? `Self-hosting · ${mode?.project ?? "this app"}`
          : "Not the Orbit repository"}
      </div>
      {selfHosting ? (
        <div className="chain-verify">
          <div className="chain-verify-title">Verification runs after each cycle</div>
          {mode?.verify_commands.map((step) => (
            <div key={step.command} className="chain-verify-row">
              <span className="chain-verify-label">{step.label}</span>
              <code>{step.command}</code>
            </div>
          ))}
        </div>
      ) : (
        <div className="chain-hint">
          Verification needs the Orbit repository, so only the agent's own judgement
          decides when a chain is done here. Open the repo that builds Orbit to get
          build-and-test gating.
        </div>
      )}

      <textarea
        value={goal}
        onChange={(e) => setGoal(e.target.value)}
        placeholder={"Goal for the agent, e.g. \u201cadd a Copy button to the status bar\u201d"}
        rows={2}
        disabled={busy}
      />
      <div className="chain-controls">
        <label>
          Max cycles
          <select
            value={maxCycles}
            onChange={(e) => setMaxCycles(Number(e.target.value))}
            disabled={busy}
          >
            {[1, 2, 3, 4, 6, 8].map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
        {running ? (
          <button className="chain-stop" onClick={() => void stop()}>
            Stop chain
          </button>
        ) : (
          <button className="chain-start" onClick={() => void start()} disabled={busy || !goal.trim()}>
            {busy ? "Starting…" : "Start chain"}
          </button>
        )}
      </div>

      {error && <div className="chain-error">{error}</div>}

      <div className="chain-log" ref={logRef}>
        {lines.length === 0 && (
          <div className="chain-empty">
            No chain run yet. The agent edits files, then the check above runs; a
            failure is fed straight back into the next cycle.
          </div>
        )}
        {lines.map((line, i) => (
          <div key={i} className="chain-log-line">
            {line}
          </div>
        ))}
      </div>

      {runs.length > 0 && (
        <div className="chain-runs">
          {runs
            .slice(-3)
            .reverse()
            .map((run) => (
              <div key={run.id} className={"chain-run status-" + run.status}>
                <span className="chain-run-status">{statusLabel(run)}</span>
                <span className="chain-run-goal" title={run.goal}>
                  {run.goal}
                </span>
              </div>
            ))}
        </div>
      )}
    </div>
  );
}

/**
 * The Harness tab: hand one task to the installed DeepSeek Harness CLI
 * (`dsh --profile headless "…"`) and stream its output. This is the second,
 * heavier agent path — it brings the Harness's own tools and plugins rather
 * than Orbit's built-in loop.
 */
function HarnessPanel({ workspace }: { workspace: string }) {
  const [status, setStatus] = useState<DshStatus | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [prompt, setPrompt] = useState("");
  const [runId, setRunId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const logRef = useRef<HTMLDivElement>(null);

  const refresh = useCallback(() => {
    ipc
      .dshStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  useEffect(refresh, [refresh]);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<DshEvent>(DSH_EVENT, (event) => {
      const e = event.payload;
      switch (e.type) {
        case "line":
          setLines((prev) => [...prev, e.line].slice(-MAX_HARNESS_LINES));
          break;
        case "exit":
          setLines((prev) =>
            [...prev, `— harness exited (code ${e.code ?? "killed"}) —`].slice(-MAX_HARNESS_LINES)
          );
          setRunId(null);
          break;
        case "error":
          setError(e.error);
          setRunId(null);
          break;
      }
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines]);

  async function run() {
    const text = prompt.trim();
    if (!text) return;
    setError(null);
    setLines([]);
    try {
      setRunId(await ipc.dshRun(text, workspace));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  if (status && !status.available) {
    return (
      <div className="harness-panel">
        <div className="harness-empty">
          <p>{status.note}</p>
          <p className="harness-muted">
            The Harness is optional: Orbit's own agent runs without it.
          </p>
          <button onClick={refresh}>Check again</button>
        </div>
      </div>
    );
  }

  return (
    <div className="harness-panel">
      <div className="harness-status">
        {status?.available
          ? `Harness · ${status.program} (profile ${status.profile})`
          : "Looking for the harness…"}
      </div>
      <textarea
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        placeholder="One task for the DeepSeek Harness, e.g. \u201cinvestigate why the build is slow\u201d"
        rows={2}
        disabled={runId !== null}
      />
      <div className="harness-controls">
        {runId ? (
          <button className="chain-stop" onClick={() => void ipc.stopDsh(runId)}>
            Stop
          </button>
        ) : (
          <button className="chain-start" onClick={() => void run()} disabled={!prompt.trim()}>
            Run in harness
          </button>
        )}
      </div>
      {error && <div className="chain-error">{error}</div>}
      <div className="chain-log" ref={logRef}>
        {lines.length === 0 && (
          <div className="chain-empty">
            No harness run yet. Output streams here as the Harness works.
          </div>
        )}
        {lines.map((line, i) => (
          <div key={i} className="chain-log-line">
            {line}
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * The right-hand agent column: the ordinary review-based chat, autonomous
 * chains, and the DeepSeek Harness.
 */
export default function AgentPane() {
  const projectRoot = useStore((s) => s.projectRoot);
  const [tab, setTab] = useState<Tab>("chat");

  if (!projectRoot) {
    return (
      <div className="agent-pane">
        <ChatPanel />
      </div>
    );
  }

  return (
    <div className="agent-pane">
      <div className="agent-tabs">
        {(
          [
            ["chat", "Chat"],
            ["chain", "Chain"],
            ["harness", "Harness"],
          ] as [Tab, string][]
        ).map(([id, label]) => (
          <button
            key={id}
            className={tab === id ? "agent-tab active" : "agent-tab"}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="agent-tab-body">
        {tab === "chat" && <ChatPanel />}
        {tab === "chain" && <ChainPanel workspace={projectRoot} />}
        {tab === "harness" && <HarnessPanel workspace={projectRoot} />}
      </div>
    </div>
  );
}
