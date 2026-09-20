import { useEffect, useState } from "react";
import { ipc } from "../ipc/invoke";
import type { FileEntry } from "../ipc/contract";
import { useStore } from "../state/store";

interface TreeNodeProps {
  entry: FileEntry;
  depth: number;
  expanded: Set<string>;
  childrenMap: Map<string, FileEntry[]>;
  onToggle: (path: string) => void;
  onOpen: (path: string) => void;
  activePath: string | null;
}

function TreeNode({ entry, depth, expanded, childrenMap, onToggle, onOpen, activePath }: TreeNodeProps) {
  const style = { paddingLeft: 8 + depth * 14 };

  if (entry.is_dir) {
    const isOpen = expanded.has(entry.path);
    const kids = childrenMap.get(entry.path) ?? [];
    return (
      <div>
        <div className="tree-row" style={style} onClick={() => onToggle(entry.path)}>
          <span className={"tree-caret" + (isOpen ? " open" : "")}>▸</span>
          <span className="tree-name">{entry.name}</span>
        </div>
        {isOpen &&
          kids.map((kid) => (
            <TreeNode
              key={kid.path}
              entry={kid}
              depth={depth + 1}
              expanded={expanded}
              childrenMap={childrenMap}
              onToggle={onToggle}
              onOpen={onOpen}
              activePath={activePath}
            />
          ))}
      </div>
    );
  }

  const active = activePath === entry.path;
  return (
    <div
      className={"tree-row" + (active ? " active" : "")}
      style={style}
      onClick={() => onOpen(entry.path)}
    >
      <span className="tree-caret" />
      <span className="tree-name">{entry.name}</span>
    </div>
  );
}

export default function FileTree() {
  const projectRoot = useStore((s) => s.projectRoot);
  const openFile = useStore((s) => s.openFile);
  const activePath = useStore((s) => s.activePath);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [childrenMap, setChildrenMap] = useState<Map<string, FileEntry[]>>(new Map());

  useEffect(() => {
    if (!projectRoot) {
      setExpanded(new Set());
      setChildrenMap(new Map());
      return;
    }
    setExpanded(new Set([projectRoot]));
    setChildrenMap(new Map());
    void ipc
      .listDir(projectRoot)
      .then((entries) => setChildrenMap(new Map([[projectRoot, entries]])))
      .catch(() => setChildrenMap(new Map()));
  }, [projectRoot]);

  async function toggle(path: string) {
    if (expanded.has(path)) {
      setExpanded((prev) => {
        const next = new Set(prev);
        next.delete(path);
        return next;
      });
      return;
    }
    if (!childrenMap.has(path)) {
      try {
        const entries = await ipc.listDir(path);
        setChildrenMap((prev) => new Map(prev).set(path, entries));
      } catch {
        return;
      }
    }
    setExpanded((prev) => new Set(prev).add(path));
  }

  if (!projectRoot) {
    return <div className="tree-empty">Open a folder to browse files</div>;
  }

  const rootEntries = childrenMap.get(projectRoot) ?? [];

  return (
    <div className="file-tree">
      {rootEntries.map((entry) => (
        <TreeNode
          key={entry.path}
          entry={entry}
          depth={0}
          expanded={expanded}
          childrenMap={childrenMap}
          onToggle={(p) => void toggle(p)}
          onOpen={(p) => void openFile(p)}
          activePath={activePath}
        />
      ))}
    </div>
  );
}
