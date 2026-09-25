import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../state/store";
import { useDiscordStore } from "../state/discordStore";
import { basename } from "../lib/path";

export default function Header() {
  const projectRoot = useStore((s) => s.projectRoot);
  const settings = useStore((s) => s.settings);
  const openProject = useStore((s) => s.openProject);
  const setSettingsOpen = useStore((s) => s.setSettingsOpen);
  const discordMode = useDiscordStore((s) => s.discordMode);
  const setDiscordMode = useDiscordStore((s) => s.setDiscordMode);

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
      <button
        className={"header-open" + (discordMode ? " active" : "")}
        onClick={() => setDiscordMode(!discordMode)}
      >
        Discord analysis
      </button>
      <div className="header-project" title={projectRoot ?? undefined}>
        {projectRoot ? basename(projectRoot) : "No folder open"}
      </div>
      <div className="header-spacer" />
      <div className="header-model" title="Jev (TypeSafe) model — powers profiling, bot detection, guardrails">
        Jev · {settings?.typesafe_model ?? "jev-latest"}
      </div>
      <div className="header-model" title="DeepSeek model — powers chat and analysis discussion">
        DeepSeek · {settings?.model ?? "deepseek-chat"}
      </div>
      <button className="header-settings" title="Settings" onClick={() => setSettingsOpen(true)}>
        ⚙
      </button>
    </header>
  );
}
