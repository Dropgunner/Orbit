import { useStore } from "../state/store";
import FileTree from "./FileTree";
import GitPanel from "./GitPanel";

export default function Sidebar() {
  const gitTab = useStore((s) => s.gitTab);
  const setGitTab = useStore((s) => s.setGitTab);

  return (
    <aside className="sidebar">
      <div className="sidebar-tabs">
        <button
          className={gitTab === "tree" ? "active" : ""}
          onClick={() => setGitTab("tree")}
        >
          Files
        </button>
        <button
          className={gitTab === "git" ? "active" : ""}
          onClick={() => setGitTab("git")}
        >
          Git
        </button>
      </div>
      <div className="sidebar-body">{gitTab === "tree" ? <FileTree /> : <GitPanel />}</div>
    </aside>
  );
}
