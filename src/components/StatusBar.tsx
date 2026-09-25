import { useStore } from "../state/store";
import { useDiscordStore } from "../state/discordStore";
import { ipc } from "../ipc/invoke";
import { isTauri } from "../lib/tauri";
import { BUILD_STAMP } from "../build";

/**
 * The always-visible bottom bar. It also carries the provider indicators
 * (Jev / DeepSeek / Discord) and the build stamp, so which models are active
 * and which build is running are visible in every mode.
 */
export default function StatusBar() {
  const statusMessage = useStore((s) => s.statusMessage);
  const setStatusMessage = useStore((s) => s.setStatusMessage);
  const gitStatus = useStore((s) => s.gitStatus);
  const streaming = useStore((s) => s.streaming);
  const settings = useStore((s) => s.settings);
  const account = useDiscordStore((s) => s.account);
  const discordMode = useDiscordStore((s) => s.discordMode);
  const setDiscordMode = useDiscordStore((s) => s.setDiscordMode);

  const tsReady = Boolean(settings?.typesafe_api_key);
  const dsReady = Boolean(settings?.api_key);
  const dcReady = Boolean(settings?.discord_token) || Boolean(account);

  /** Start the local web server and open this UI in the default browser. */
  async function openInBrowser() {
    try {
      const url = await ipc.startWebUi();
      setStatusMessage(`Web UI: ${url}`);
    } catch (e) {
      setStatusMessage(`Web UI failed: ${String(e)}`);
    }
  }

  return (
    <footer className="statusbar">
      <span className="status-msg">{statusMessage}</span>
      <span className="status-spacer" />
      <span
        className="status-provider"
        title="Jev (TypeSafe) — profiling, bot detection, guardrails"
      >
        <span className={`discord-dot ${tsReady ? "ok" : "warn"}`} />
        Jev {settings?.typesafe_model ?? "jev-latest"}
      </span>
      <span className="status-provider" title="DeepSeek — chat and analysis discussion">
        <span className={`discord-dot ${dsReady ? "ok" : "warn"}`} />
        DeepSeek {settings?.model ?? "deepseek-chat"}
      </span>
      <span className="status-provider" title="Discord — live browsing and exports">
        <span className={`discord-dot ${dcReady ? "ok" : "warn"}`} />
        Discord {account ? `@${account.username}` : "not connected"}
      </span>
      {isTauri() ? (
        <>
          <button
            className="status-toggle"
            onClick={() => void openInBrowser()}
            title="Serve this UI to your browser on 127.0.0.1"
          >
            Open in browser
          </button>
          <button className="status-toggle" onClick={() => setDiscordMode(!discordMode)}>
            {discordMode ? "IDE" : "Discord analysis"}
          </button>
        </>
      ) : (
        <span className="status-provider" title="Running in your browser via the local Orbit server">
          <span className="discord-dot ok" />
          web UI
        </span>
      )}
      {streaming && <span className="status-streaming">● agent running</span>}
      {gitStatus && <span className="status-branch">⎇ {gitStatus.branch}</span>}
      <span className="status-build" title="Build stamp — identifies this build">
        {BUILD_STAMP}
      </span>
    </footer>
  );
}
