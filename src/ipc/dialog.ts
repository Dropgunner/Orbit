import { open, save } from "@tauri-apps/plugin-dialog";
import { isTauri } from "../lib/tauri";
import { ipc } from "./invoke";

/**
 * Pick a text file and return its name and contents. Uses the Tauri file dialog
 * on desktop and a hidden `<input type="file">` in a browser, so callers can
 * always import through the same contents-based command.
 */
export async function pickTextFile(): Promise<{ name: string; content: string } | null> {
  if (isTauri()) {
    const path = await open({
      multiple: false,
      title: "Import a Discord export",
      filters: [{ name: "Discord export", extensions: ["json", "html", "txt", "md", "log"] }],
    });
    if (typeof path !== "string") return null;
    const content = await ipc.readFile(path);
    return { name: path.split(/[\\/]/).pop() ?? "discord.json", content };
  }

  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,.html,.txt,.md,.log";
    input.onchange = () => {
      const file = input.files?.[0];
      if (!file) {
        resolve(null);
        return;
      }
      const reader = new FileReader();
      reader.onload = () => resolve({ name: file.name, content: String(reader.result ?? "") });
      reader.onerror = () => resolve(null);
      reader.readAsText(file);
    };
    input.click();
  });
}

/** Save JSON to a file: a save dialog on desktop, a download in the browser. */
export async function saveJsonFile(defaultName: string, value: unknown): Promise<string | null> {
  const text = JSON.stringify(value, null, 2);
  if (isTauri()) {
    const path = await save({
      title: "Save Discord data as JSON",
      defaultPath: defaultName,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return null;
    await ipc.writeFile(path, text);
    return path;
  }
  const blob = new Blob([text], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = defaultName;
  anchor.click();
  URL.revokeObjectURL(url);
  return defaultName;
}
