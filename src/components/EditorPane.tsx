import { useMemo, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { oneDark } from "@codemirror/theme-one-dark";
import { keymap } from "@codemirror/view";
import { useStore } from "../state/store";
import { languageFor } from "../lib/languages";
import { basename } from "../lib/path";
import DiffView from "./DiffView";
import type { PendingEdit } from "../ipc/contract";

function ReviewPanel({
  pendingEdits,
  applyEdit,
  discardEdit,
  applyAllEdits,
}: {
  pendingEdits: PendingEdit[];
  applyEdit: (id: string) => Promise<void>;
  discardEdit: (id: string) => Promise<void>;
  applyAllEdits: () => Promise<void>;
}) {
  const [openId, setOpenId] = useState<string | null>(null);

  return (
    <div className="review-panel">
      <div className="review-header">
        <span>{pendingEdits.length} proposed change{pendingEdits.length === 1 ? "" : "s"} awaiting review</span>
        <button className="primary" onClick={() => void applyAllEdits()}>
          Accept all
        </button>
      </div>
      <div className="review-list">
        {pendingEdits.map((edit) => {
          const isOpen = openId === edit.id;
          return (
            <div key={edit.id} className="review-item">
              <div
                className="review-item-header"
                onClick={() => setOpenId(isOpen ? null : edit.id)}
              >
                <span className="review-caret">{isOpen ? "▾" : "▸"}</span>
                <span className="review-path" title={edit.path}>
                  {basename(edit.path)}
                </span>
                <span className="review-desc">{edit.description}</span>
              </div>
              {isOpen && (
                <div className="review-body">
                  <DiffView before={edit.before} after={edit.after} />
                  <div className="review-actions">
                    <button onClick={() => void discardEdit(edit.id)}>Reject</button>
                    <button className="primary" onClick={() => void applyEdit(edit.id)}>
                      Accept
                    </button>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export default function EditorPane() {
  const openFiles = useStore((s) => s.openFiles);
  const activePath = useStore((s) => s.activePath);
  const setActivePath = useStore((s) => s.setActivePath);
  const updateFileContent = useStore((s) => s.updateFileContent);
  const saveFile = useStore((s) => s.saveFile);
  const closeFile = useStore((s) => s.closeFile);
  const pendingEdits = useStore((s) => s.pendingEdits);
  const applyEdit = useStore((s) => s.applyEdit);
  const discardEdit = useStore((s) => s.discardEdit);
  const applyAllEdits = useStore((s) => s.applyAllEdits);

  const active = openFiles.find((f) => f.path === activePath);

  const extensions = useMemo(() => {
    const lang = active ? languageFor(active.path) : null;
    const save = keymap.of([
      {
        key: "Mod-s",
        run: () => {
          if (activePath) void saveFile(activePath);
          return true;
        },
      },
    ]);
    return [save, ...(lang ? [lang] : [])];
  }, [active?.path, activePath, saveFile]);

  return (
    <div className="editor-pane">
      <div className="tabs">
        {openFiles.map((f) => {
          const isActive = f.path === activePath;
          const dirty = f.content !== f.savedContent;
          return (
            <div
              key={f.path}
              className={"tab" + (isActive ? " active" : "")}
              onClick={() => setActivePath(f.path)}
            >
              <span className="tab-name">{basename(f.path)}</span>
              {dirty && <span className="tab-dirty">●</span>}
              <button
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation();
                  closeFile(f.path);
                }}
              >
                ×
              </button>
            </div>
          );
        })}
      </div>

      {active ? (
        <CodeMirror
          className="cm-editor"
          value={active.content}
          height="100%"
          theme={oneDark}
          extensions={extensions}
          onChange={(value) => updateFileContent(active.path, value)}
        />
      ) : (
        <div className="empty-editor">Select a file to edit</div>
      )}

      {pendingEdits.length > 0 && (
        <ReviewPanel
          pendingEdits={pendingEdits}
          applyEdit={applyEdit}
          discardEdit={discardEdit}
          applyAllEdits={applyAllEdits}
        />
      )}
    </div>
  );
}
