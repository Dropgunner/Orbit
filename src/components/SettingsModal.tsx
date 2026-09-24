import { useEffect, useState } from "react";
import { useStore } from "../state/store";
import type { Settings } from "../ipc/contract";

export default function SettingsModal() {
  const settings = useStore((s) => s.settings);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const setSettingsOpen = useStore((s) => s.setSettingsOpen);
  const saveSettings = useStore((s) => s.saveSettings);
  const [form, setForm] = useState<Settings | null>(null);

  useEffect(() => {
    setForm(settings);
  }, [settings, settingsOpen]);

  if (!settingsOpen || !form) return null;

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    setForm((f) => (f ? { ...f, [key]: value } : f));
  }

  return (
    <div className="modal-backdrop" onClick={() => setSettingsOpen(false)}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Settings</h2>
        <label className="field">
          <span>Base URL</span>
          <input
            value={form.base_url}
            onChange={(e) => update("base_url", e.target.value)}
            placeholder="https://api.deepseek.com"
          />
        </label>
        <label className="field">
          <span>Model</span>
          <input
            value={form.model}
            onChange={(e) => update("model", e.target.value)}
            placeholder="deepseek-chat"
          />
        </label>
        <label className="field">
          <span>Temperature</span>
          <input
            type="number"
            step="0.1"
            min="0"
            max="2"
            value={form.temperature}
            onChange={(e) => update("temperature", Number(e.target.value))}
          />
        </label>
        <label className="field">
          <span>API Key</span>
          <input
            type="password"
            value={form.api_key ?? ""}
            onChange={(e) => update("api_key", e.target.value || null)}
            placeholder="sk-… (leave empty for local Ollama)"
          />
        </label>
        <p className="modal-hint">
          DeepSeek/OpenAI needs an API key (stored in the app config dir, or set
          ORBIT_API_KEY / DEEPSEEK_API_KEY). For a local model, set Base URL to
          http://localhost:11434/v1 and leave the key empty (Ollama).
        </p>
        <div className="modal-actions">
          <button onClick={() => setSettingsOpen(false)}>Cancel</button>
          <button className="primary" onClick={() => void saveSettings(form)}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
