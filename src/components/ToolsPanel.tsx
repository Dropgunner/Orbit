import { useState } from "react";
import type { ReactNode } from "react";
import { ipc } from "../ipc/invoke";
import { useStore } from "../state/store";
import type {
  ClassifyResult,
  CompactEntry,
  CompactResult,
  GenerateResult,
  NoulResult,
  PruneResult,
  PsychProfileResult,
  ScoreResult,
} from "../ipc/contract";

type ToolId = "classify" | "score" | "psych" | "noul" | "prune" | "compact" | "generate";

const TOOLS: readonly { id: ToolId; name: string; blurb: string }[] = [
  { id: "classify", name: "Classify", blurb: "Pick one of your labels, with probabilities." },
  { id: "score", name: "Score", blurb: "Score text against an ordered rubric." },
  { id: "psych", name: "Psych profile", blurb: "Score one message across 35 dimensions in a single call." },
  { id: "noul", name: "Yes / no gate", blurb: "Return the probability of a yes answer." },
  { id: "prune", name: "Prune", blurb: "Trim a long output; dropped runs become “[N lines omitted]”." },
  { id: "compact", name: "Compact", blurb: "Drop or truncate stale tool results, keep the rest verbatim." },
  { id: "generate", name: "Generate", blurb: "Greedy next-token choice from a candidate set." },
];

interface ToolValueMap {
  classify: ClassifyResult;
  score: ScoreResult;
  psych: PsychProfileResult;
  noul: NoulResult;
  prune: PruneResult;
  compact: CompactResult;
  generate: GenerateResult;
}

type ToolResult = { [K in ToolId]: { kind: K; value: ToolValueMap[K] } }[ToolId];

function splitList(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function splitLines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function fmt(value: number, digits = 2): string {
  return value.toFixed(digits);
}

/** A labeled input row. */
function Field({
  label,
  value,
  onChange,
  placeholder,
  type = "text",
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: string;
}) {
  return (
    <label className="tool-field">
      <span className="tool-label">{label}</span>
      <input
        className="tool-input"
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

/** A labeled textarea row. */
function Area({
  label,
  value,
  onChange,
  placeholder,
  rows = 4,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  rows?: number;
}) {
  return (
    <label className="tool-field">
      <span className="tool-label">{label}</span>
      <textarea
        className="tool-area"
        rows={rows}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

/** A compact 0–1 probability bar. */
function ProbBar({ value, label, hint }: { value: number; label: string; hint?: string }) {
  const pct = Math.round(Math.max(0, Math.min(1, value)) * 100);
  return (
    <div className="prob-row" title={hint ?? value.toFixed(3)}>
      <span className="prob-label">{label}</span>
      <div className="prob-track">
        <div className="prob-fill" style={{ width: `${pct}%` }} />
      </div>
      <span className="prob-value">{fmt(value)}</span>
    </div>
  );
}

function usageLabel(result: { model: string; usage: { input_tokens: number; output_tokens: number } }): string {
  return `${result.model} · ${result.usage.input_tokens} in / ${result.usage.output_tokens} out tokens`;
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard may be unavailable in some webviews; a silent no-op is fine.
      return;
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  }
  return (
    <button className="tool-copy" onClick={() => void copy()}>
      {copied ? "Copied" : "Copy"}
    </button>
  );
}

function ResultShell({
  title,
  result,
  children,
}: {
  title: string;
  result: { model: string; usage: { input_tokens: number; output_tokens: number } };
  children: ReactNode;
}) {
  return (
    <div className="tool-result">
      <div className="tool-result-head">
        <strong>{title}</strong>
        <span className="tool-usage">{usageLabel(result)}</span>
      </div>
      {children}
    </div>
  );
}

export default function ToolsPanel() {
  const [tool, setTool] = useState<ToolId>("classify");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ToolResult | null>(null);
  const settings = useStore((s) => s.settings);
  const model = settings?.typesafe_model ?? "jev-latest";

  async function run<K extends ToolId>(kind: K, fn: () => Promise<ToolValueMap[K]>) {
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      const value = await fn();
      setResult({ kind, value } as ToolResult);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="tools-panel">
      <div className="discord-section-title">Jev toolkit · {model}</div>

      <div className="tools-nav">
        {TOOLS.map((t) => (
          <button
            key={t.id}
            className={"tool-chip" + (tool === t.id ? " active" : "")}
            onClick={() => {
              setTool(t.id);
              setResult(null);
              setError(null);
            }}
            title={t.blurb}
          >
            {t.name}
          </button>
        ))}
      </div>

      <div className="tool-card">
        <div className="tool-head">
          <span className="tool-name">{TOOLS.find((t) => t.id === tool)?.name}</span>
          <span className="tool-blurb">{TOOLS.find((t) => t.id === tool)?.blurb}</span>
        </div>

        {tool === "classify" && <ClassifyForm run={run} />}
        {tool === "score" && <ScoreForm run={run} />}
        {tool === "psych" && <PsychForm run={run} />}
        {tool === "noul" && <NoulForm run={run} />}
        {tool === "prune" && <PruneForm run={run} />}
        {tool === "compact" && <CompactForm run={run} />}
        {tool === "generate" && <GenerateForm run={run} />}
      </div>

      {loading && <div className="discord-loading">Working…</div>}
      {error && <div className="discord-error">{error}</div>}
      {result && <ResultView result={result} />}
    </div>
  );
}

type Runner = <K extends ToolId>(kind: K, fn: () => Promise<ToolValueMap[K]>) => Promise<void>;

function ClassifyForm({ run }: { run: Runner }) {
  const [text, setText] = useState("");
  const [instructions, setInstructions] = useState("");
  const [labels, setLabels] = useState("");
  return (
    <div className="tool-body">
      <Area label="Text" value={text} onChange={setText} placeholder="Text to classify…" />
      <Field label="Instructions" value={instructions} onChange={setInstructions} placeholder="e.g. Which category does this support ticket belong to?" />
      <Field label="Labels" value={labels} onChange={setLabels} placeholder="billing, account, other (comma-separated, at least two)" />
      <div className="tool-actions">
        <button className="primary" onClick={() => void run("classify", () => ipc.classify(text, instructions, splitList(labels)))}>
          Classify
        </button>
      </div>
    </div>
  );
}

function ScoreForm({ run }: { run: Runner }) {
  const [text, setText] = useState("");
  const [instructions, setInstructions] = useState("");
  const [rubric, setRubric] = useState("");
  return (
    <div className="tool-body">
      <Area label="Text" value={text} onChange={setText} placeholder="Text to score…" />
      <Field label="Instructions" value={instructions} onChange={setInstructions} placeholder="e.g. How helpful is this answer?" />
      <Area label="Rubric" value={rubric} onChange={setRubric} placeholder={"Level 0 description\nLevel 1 description\nLevel 2 description"} rows={5} />
      <div className="tool-actions">
        <button className="primary" onClick={() => void run("score", () => ipc.score(text, instructions, splitLines(rubric)))}>
          Score
        </button>
      </div>
    </div>
  );
}

function PsychForm({ run }: { run: Runner }) {
  const [text, setText] = useState("");
  return (
    <div className="tool-body">
      <Area label="Text" value={text} onChange={setText} placeholder="A single message or short transcript…" rows={5} />
      <div className="tool-actions">
        <button className="primary" onClick={() => void run("psych", () => ipc.psychProfile(text))}>
          Profile
        </button>
      </div>
    </div>
  );
}

function NoulForm({ run }: { run: Runner }) {
  const [text, setText] = useState("");
  const [instructions, setInstructions] = useState("");
  const [yes, setYes] = useState("");
  const [no, setNo] = useState("");
  return (
    <div className="tool-body">
      <Area label="Text" value={text} onChange={setText} placeholder="Text to evaluate…" />
      <Field label="Question" value={instructions} onChange={setInstructions} placeholder="e.g. Is this a billing issue?" />
      <div className="tool-row">
        <Field label="Yes means" value={yes} onChange={setYes} placeholder="optional" />
        <Field label="No means" value={no} onChange={setNo} placeholder="optional" />
      </div>
      <div className="tool-actions">
        <button className="primary" onClick={() => void run("noul", () => ipc.noul(text, instructions, yes || null, no || null))}>
          Ask
        </button>
      </div>
    </div>
  );
}

function PruneForm({ run }: { run: Runner }) {
  const [text, setText] = useState("");
  const [goal, setGoal] = useState("");
  const [chunkLines, setChunkLines] = useState("20");
  const [keepThreshold, setKeepThreshold] = useState("0.5");
  return (
    <div className="tool-body">
      <Area label="Text" value={text} onChange={setText} placeholder="Long output to prune…" rows={6} />
      <Field label="Goal" value={goal} onChange={setGoal} placeholder="What you need from this output (optional)" />
      <div className="tool-row">
        <Field label="Chunk lines" value={chunkLines} onChange={setChunkLines} type="number" placeholder="20" />
        <Field label="Keep threshold" value={keepThreshold} onChange={setKeepThreshold} type="number" placeholder="0.5" />
      </div>
      <div className="tool-actions">
        <button className="primary" onClick={() => void run("prune", () => ipc.prune(text, goal || null, Number(chunkLines) || null, Number(keepThreshold) || null))}>
          Prune
        </button>
      </div>
    </div>
  );
}

function CompactForm({ run }: { run: Runner }) {
  const [entries, setEntries] = useState("");
  const [goal, setGoal] = useState("");
  const [preserveRecent, setPreserveRecent] = useState("6");
  const [keepThreshold, setKeepThreshold] = useState("0.5");
  return (
    <div className="tool-body">
      <Area
        label="Entries"
        value={entries}
        onChange={setEntries}
        placeholder={'[{"id":"t1","text":"Read file","result":"contents"},{"id":"m1","text":"a plain message"}]'}
        rows={6}
      />
      <Field label="Goal" value={goal} onChange={setGoal} placeholder="The ongoing task (optional)" />
      <div className="tool-row">
        <Field label="Preserve recent" value={preserveRecent} onChange={setPreserveRecent} type="number" placeholder="6" />
        <Field label="Keep threshold" value={keepThreshold} onChange={setKeepThreshold} type="number" placeholder="0.5" />
      </div>
      <div className="tool-actions">
        <button
          className="primary"
          onClick={() =>
            void run("compact", () => {
              let parsed: unknown;
              try {
                parsed = JSON.parse(entries);
              } catch {
                throw new Error("Entries must be a JSON array of {id, text, result?} objects.");
              }
              const list = Array.isArray(parsed) ? (parsed as CompactEntry[]) : [];
              return ipc.compact(list, goal || null, Number(preserveRecent) || null, Number(keepThreshold) || null);
            })
          }
        >
          Compact
        </button>
      </div>
    </div>
  );
}

function GenerateForm({ run }: { run: Runner }) {
  const [prompt, setPrompt] = useState("");
  const [candidates, setCandidates] = useState("");
  const [maxTokens, setMaxTokens] = useState("10");
  const [stop, setStop] = useState("");
  return (
    <div className="tool-body">
      <Area label="Prompt" value={prompt} onChange={setPrompt} placeholder="Text to continue…" />
      <Field label="Candidates" value={candidates} onChange={setCandidates} placeholder="answer, is, 42, . (comma-separated, at least two)" />
      <div className="tool-row">
        <Field label="Max tokens" value={maxTokens} onChange={setMaxTokens} type="number" placeholder="10" />
        <Field label="Stop tokens" value={stop} onChange={setStop} placeholder=". (comma-separated, optional)" />
      </div>
      <div className="tool-actions">
        <button className="primary" onClick={() => void run("generate", () => ipc.generate(prompt, splitList(candidates), Number(maxTokens) || null, splitList(stop).length ? splitList(stop) : null))}>
          Generate
        </button>
      </div>
    </div>
  );
}

function ResultView({ result }: { result: ToolResult }) {
  switch (result.kind) {
    case "classify":
      return (
        <ResultShell title="Classification" result={result.value}>
          <div className="tool-choice">{result.value.choice}</div>
          <div className="prob-list">
            {result.value.probabilities.map((p) => (
              <ProbBar key={p.label} label={p.label} value={p.probability} />
            ))}
          </div>
        </ResultShell>
      );
    case "score":
      return (
        <ResultShell title={`Score ${fmt(result.value.score)}`} result={result.value}>
          <div className="prob-list">
            {result.value.probabilities.map((p) => (
              <ProbBar key={p.level} label={`${p.level} · ${p.description}`} value={p.probability} />
            ))}
          </div>
        </ResultShell>
      );
    case "psych":
      return (
        <ResultShell title={`Psych profile (${result.value.dimensions.length} dimensions)`} result={result.value}>
          <div className="tool-table-wrap">
            <table className="discord-table">
              <thead>
                <tr>
                  <th>Dimension</th>
                  <th>Score</th>
                  <th>Conf</th>
                </tr>
              </thead>
              <tbody>
                {result.value.dimensions.map((d) => (
                  <tr key={d.id}>
                    <td title={`${d.direction} · ${d.category}`} className={d.polarity === "inverted-risk" ? "risk" : d.polarity === "desirable" ? "good" : ""}>
                      {d.label}
                    </td>
                    <td>{fmt(d.score)}</td>
                    <td>{fmt(d.confidence)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </ResultShell>
      );
    case "noul":
      return (
        <ResultShell title="Yes probability" result={result.value}>
          <ProbBar label="yes" value={result.value.yes_probability} />
        </ResultShell>
      );
    case "prune":
      return (
        <ResultShell title={`Pruned · ${result.value.kept_chunks} kept / ${result.value.dropped_chunks} dropped`} result={result.value}>
          <div className="tool-output">
            <CopyButton text={result.value.pruned} />
            <pre>{result.value.pruned}</pre>
          </div>
        </ResultShell>
      );
    case "compact":
      return (
        <ResultShell title={`Compacted · ${result.value.stats.before} → ${result.value.stats.after} entries`} result={result.value}>
          <div className="tool-stats">
            <span>dropped {result.value.stats.dropped}</span>
            <span>truncated {result.value.stats.truncated}</span>
          </div>
          {result.value.decisions.length > 0 && (
            <div className="prob-list">
              {result.value.decisions.map((d) => (
                <div key={d.id} className="tool-decision">
                  <span className="prob-label">{d.id}</span>
                  <span className="tool-action-pill">{d.action}</span>
                </div>
              ))}
            </div>
          )}
          <div className="tool-output">
            <CopyButton text={JSON.stringify(result.value.entries, null, 2)} />
            <pre>{JSON.stringify(result.value.entries, null, 2)}</pre>
          </div>
        </ResultShell>
      );
    case "generate":
      return (
        <ResultShell title="Generated" result={result.value}>
          <div className="tool-output">
            <CopyButton text={result.value.text} />
            <pre>{result.value.text}</pre>
          </div>
          {result.value.tokens.length > 0 && (
            <div className="tool-tokens">{result.value.tokens.map((t, i) => `#${i + 1} ${t}`).join(" · ")}</div>
          )}
        </ResultShell>
      );
  }
}
