import { useState } from "react";
import { useStore } from "../state/store";
import { useDiscordStore } from "../state/discordStore";
import { isTauri } from "../lib/tauri";
import { ipc } from "../ipc/invoke";

/**
 * Step-by-step Discord token setup. Discord Web no longer keeps the token in
 * localStorage, so the reliable source is the `authorization` request header of
 * any `/api/` call; the Console snippet is the fallback.
 */
const CONSOLE_SNIPPET =
  "(webpackChunkdiscord_app.push([[''],{},e=>{m=[];for(let c in e.c)m.push(e.c[c])}]),m)" +
  ".find(m=>m?.exports?.default?.getToken!==void 0).exports.default.getToken()";

export default function DiscordTokenWizard({ onClose }: { onClose: () => void }) {
  const settings = useStore((s) => s.settings);
  const saveSettings = useStore((s) => s.saveSettings);
  const account = useDiscordStore((s) => s.account);
  const connectDiscord = useDiscordStore((s) => s.connectDiscord);
  const [token, setToken] = useState(settings?.discord_token ?? "");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  function openDiscord() {
    if (isTauri()) {
      void ipc.openDiscordWeb();
    } else {
      window.open("https://discord.com/app", "_blank", "noopener,noreferrer");
    }
  }

  async function copySnippet() {
    try {
      await navigator.clipboard.writeText(CONSOLE_SNIPPET);
      setCopied(true);
    } catch {
      setCopied(false);
      setResult("Copy failed — select the snippet and copy it manually.");
    }
  }

  async function validate() {
    const trimmed = token.trim();
    if (!trimmed) {
      setResult("Paste a token first.");
      return;
    }
    if (!settings) {
      setResult("Settings are still loading — try again in a moment.");
      return;
    }
    setBusy(true);
    setResult(null);
    try {
      // Persist first: the backend reads the token from saved settings.
      await saveSettings({ ...settings, discord_token: trimmed });
      await connectDiscord();
      const connected = useDiscordStore.getState().account;
      setResult(
        connected
          ? `Connected as @${connected.username}. Pick a server on the left.`
          : "The token was saved but Discord rejected it. Re-copy it and try again."
      );
    } catch (e) {
      setResult(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="discord-wizard">
      <div className="discord-wizard-head">
        <strong>Connect Discord</strong>
        <button className="discord-clear" onClick={onClose}>
          Close
        </button>
      </div>

      <ol className="discord-wizard-steps">
        <li>
          <button onClick={openDiscord}>Open Discord Web</button>
          <span className="discord-hint"> Sign in with your own account.</span>
        </li>
        <li>
          Open DevTools there (<kbd>⌘⌥I</kbd> or <kbd>F12</kbd>), switch to the{" "}
          <strong>Network</strong> tab, and type <code>api</code> in its filter box.
        </li>
        <li>
          Click a request to <code>discord.com/api/v9/…</code> (for example{" "}
          <code>messages</code> or <code>science</code>). Static <code>/assets/…</code>{" "}
          requests have no token.
        </li>
        <li>
          In the right pane open <strong>Headers</strong> and scroll <em>past</em> “Response
          headers” to <strong>Request headers</strong>. Copy the{" "}
          <code>authorization</code> value.
        </li>
      </ol>

      <details className="discord-wizard-alt">
        <summary>No authorization header? Use the Console instead</summary>
        <p className="discord-hint">
          Paste this into the Console tab of that same DevTools window and press Enter. It
          prints the token. (Discord no longer stores it in Local Storage.)
        </p>
        <pre className="discord-snippet">{CONSOLE_SNIPPET}</pre>
        <button onClick={() => void copySnippet()}>{copied ? "Copied" : "Copy snippet"}</button>
      </details>

      <label className="field">
        <span>User token</span>
        <input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder="Paste the authorization value"
        />
      </label>

      <div className="discord-actions">
        <button className="primary" onClick={() => void validate()} disabled={busy}>
          {busy ? "Checking…" : account ? "Re-check token" : "Validate & connect"}
        </button>
        {account && <span className="discord-hint">Currently @{account.username}</span>}
      </div>

      {result && (
        <div className={account ? "discord-notice" : "discord-error"}>{result}</div>
      )}

      <p className="discord-caveat">
        A user token grants full access to your account and automating it is against
        Discord’s ToS. It stays in this app’s config directory; don’t paste it anywhere else
        (including screenshots).
      </p>
    </div>
  );
}
