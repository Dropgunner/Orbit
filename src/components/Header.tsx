import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../state/store";
import { basename } from "../lib/path";

export default function Header() {
  const projectRoot = useStore((s) => s.projectRoot);
  const settings = useStore((s) => s.settings);
  const openProject = useStore((s) => s.openProject);
  const setSettingsOpen = useStore((s) => s.setSettingsOpen);

  async function pickFolder() {
    const dir = await open({ directory: true, multiple: false, title: "Open project folder" });
    if (typeof dir === "string") {
      void openProject(dir);
    }
  }

  return (
    <header className="header">
      <div className="header-brand">Orbit</div>
      <button className="header-open" onClick={() => void pickFolder()}>
        Open Folder
      </button>
      <div className="header-project" title={projectRoot ?? undefined}>
        {projectRoot ? basename(projectRoot) : "No folder open"}
      </div>
      <div className="header-spacer" />
      <div className="header-model">{settings?.model ?? "deepseek-chat"}</div>
      <button className="header-settings" title="Settings" onClick={() => setSettingsOpen(true)}>
        ⚙
      </button>
    </header>
  );
}
