import { useEffect, useState } from "react";
import { useStore } from "../state/store";
import { useDiscordStore } from "../state/discordStore";
import type { Settings } from "../ipc/contract";

/**
 * Inline provider configuration for the Discord workspace: Jev (TypeSafe),
 * DeepSeek, and Discord, each with its model/key and a ready indicator. Saves
 * through the same settings store the Settings modal uses.
 */
export default function ProvidersPanel() {
  const settings = useStore((s) => s.settings);
  const saveSettings = useStore((s) => s.saveSettings);
  const account = useDiscordStore((s) => s.account);
  const connectDiscord = useDiscordStore((s) => s.connectDiscord);
  const [form, setForm] = useState<Settings | null>(settings);

  useEffect(() => {
    setForm(settings);
  }, [settings]);

  if (!form) {
    return <div className="providers-panel">Loading settings…</div>;
  }

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    setForm((f) => (f ? { ...f, [key]: value } : f));
  }

  return (
    <div className="providers-panel">
      <div className="provider-card">
        <div className="provider-card-head">
          <span
            className={`discord-dot ${form.typesafe_api_key || form.typesafe_api_key_set ? "ok" : "warn"}`}
          />
          <strong>Jev (TypeSafe)</strong>
          <span className="provider-role">profiling · bot detection · guardrails</span>
        </div>
        <label className="field">
          <span>Model</span>
          <input
            value={form.typesafe_model}
            onChange={(e) => update("typesafe_model", e.target.value)}
            placeholder="jev-latest"
          />
        </label>
        <label className="field">
          <span>API key</span>
          <input
            type="password"
            value={form.typesafe_api_key ?? ""}
            onChange={(e) => update("typesafe_api_key", e.target.value || null)}
            placeholder={form.typesafe_api_key_set ? "Stored — type to replace" : "TYPESAFE_API_KEY"}
          />
        </label>
        <label className="field">
          <span>Base URL</span>
          <input
            value={form.typesafe_base_url}
            onChange={(e) => update("typesafe_base_url", e.target.value)}
            placeholder="https://api.typesafe.ai"
          />
        </label>
      </div>

      <div className="provider-card">
        <div className="provider-card-head">
          <span className={`discord-dot ${form.api_key || form.api_key_set ? "ok" : "warn"}`} />
          <strong>DeepSeek</strong>
          <span className="provider-role">chat · analysis discussion</span>
        </div>
        <label className="field">
          <span>Model</span>
          <input
            value={form.model}
            onChange={(e) => update("model", e.target.value)}
            placeholder="deepseek-chat"
          />
        </label>
        <label className="field">
          <span>API key</span>
          <input
            type="password"
            value={form.api_key ?? ""}
            onChange={(e) => update("api_key", e.target.value || null)}
            placeholder={form.api_key_set ? "Stored — type to replace" : "DEEPSEEK_API_KEY"}
          />
        </label>
        <label className="field">
          <span>Base URL</span>
          <input
            value={form.base_url}
            onChange={(e) => update("base_url", e.target.value)}
            placeholder="https://api.deepseek.com"
          />
        </label>
      </div>

      <div className="provider-card">
        <div className="provider-card-head">
          <span
            className={`discord-dot ${form.discord_token || form.discord_token_set || account ? "ok" : "warn"}`}
          />
          <strong>Discord</strong>
          <span className="provider-role">
            {account ? `connected as @${account.username}` : "live browsing · DCE exports"}
          </span>
        </div>
        <label className="field">
          <span>User token</span>
          <input
            type="password"
            value={form.discord_token ?? ""}
            onChange={(e) => update("discord_token", e.target.value || null)}
            placeholder={form.discord_token_set ? "Stored — type to replace" : "DISCORD_TOKEN"}
          />
        </label>
        <button onClick={() => void connectDiscord()}>Test connection</button>
      </div>

      <div className="providers-actions">
        <span className="providers-hint">
          Keys live in the app config dir; env vars (TYPESAFE_API_KEY, DEEPSEEK_API_KEY, DISCORD_TOKEN) override them.
        </span>
        <button className="primary" onClick={() => void saveSettings(form)}>
          Save providers
        </button>
      </div>
    </div>
  );
}
