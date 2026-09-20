import { useState } from "react";
import { useStore } from "../state/store";
import type { GitStatusEntry } from "../ipc/contract";

function GitRow({
  entry,
  action,
  onAction,
}: {
  entry: GitStatusEntry;
  action: "stage" | "unstage";
  onAction: () => void;
}) {
  const code =
    entry.index === "?" ? "U" : entry.worktree !== " " ? entry.worktree : entry.index;
  return (
    <div className="git-row">
      <span className={"git-code code-" + code}>{code}</span>
      <span className="git-path" title={entry.path}>
        {entry.path}
      </span>
      <button className="git-action" onClick={onAction}>
        {action === "stage" ? "+" : "−"}
      </button>
    </div>
  );
}

export default function GitPanel() {
  const gitStatus = useStore((s) => s.gitStatus);
  const gitDiff = useStore((s) => s.gitDiff);
  const stagePaths = useStore((s) => s.stagePaths);
  const unstagePaths = useStore((s) => s.unstagePaths);
  const commit = useStore((s) => s.commit);
  const [message, setMessage] = useState("");

  if (!gitStatus) {
    return <div className="git-empty">Not a git repository.</div>;
  }

  const staged = gitStatus.entries.filter((e) => e.index !== " " && e.index !== "?");
  const unstaged = gitStatus.entries.filter((e) => e.index === "?" || e.worktree !== " ");

  async function doCommit() {
    const msg = message.trim();
    if (!msg) return;
    try {
      await commit(msg);
      setMessage("");
    } catch {
      /* status bar already shows the error */
    }
  }

  return (
    <div className="git-panel">
      <div className="git-branch">⎇ {gitStatus.branch}</div>

      <div className="git-section">
        <div className="git-section-title">Staged ({staged.length})</div>
        {staged.map((e) => (
          <GitRow
            key={"s" + e.path}
            entry={e}
            action="unstage"
            onAction={() => void unstagePaths([e.path])}
          />
        ))}
        {staged.length === 0 && <div className="git-none">No staged changes</div>}
      </div>

      <div className="git-section">
        <div className="git-section-title">Changes ({unstaged.length})</div>
        {unstaged.map((e) => (
          <GitRow
            key={"u" + e.path}
            entry={e}
            action="stage"
            onAction={() => void stagePaths([e.path])}
          />
        ))}
        {unstaged.length === 0 && <div className="git-none">No unstaged changes</div>}
      </div>

      <div className="git-commit">
        <textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          placeholder="Commit message"
          rows={2}
        />
        <button disabled={!message.trim() || staged.length === 0} onClick={() => void doCommit()}>
          Commit
        </button>
      </div>

      <div className="git-diff">
        <div className="git-section-title">Unstaged diff</div>
        <pre>{gitDiff || "(no diff)"}</pre>
      </div>
    </div>
  );
}
