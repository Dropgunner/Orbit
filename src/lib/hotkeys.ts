import { useEffect, useRef } from "react";

/** A combo string (e.g. "mod+1", "mod+shift+k", "?", "escape") to its handler. */
export type HotkeyMap = Record<string, (event: KeyboardEvent) => void>;

/** True when the event target is a text field, so plain-key hotkeys stay out of the way. */
function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || !el.tagName) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable;
}

/** Whether an event matches a combo. `mod` is Cmd on macOS, Ctrl elsewhere. */
export function matchesCombo(combo: string, event: KeyboardEvent): boolean {
  if (combo === "?") return event.key === "?";
  const parts = combo.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  const wantsMod = parts.includes("mod");
  const wantsShift = parts.includes("shift");
  const wantsAlt = parts.includes("alt");
  const mod = event.metaKey || event.ctrlKey;
  if (wantsMod !== mod) return false;
  if (wantsShift !== event.shiftKey) return false;
  if (wantsAlt !== event.altKey) return false;
  return event.key.toLowerCase() === key;
}

/**
 * Bind global keyboard shortcuts. Plain-key combos are ignored while a text
 * field has focus (except `escape`); `mod` combos always fire.
 */
export function useHotkeys(map: HotkeyMap, enabled = true): void {
  const ref = useRef(map);
  ref.current = map;

  useEffect(() => {
    if (!enabled) return;
    function onKeyDown(event: KeyboardEvent) {
      for (const [combo, handler] of Object.entries(ref.current)) {
        const isPlain = !combo.includes("mod+") && combo !== "escape";
        if (isPlain && isTyping(event.target)) continue;
        if (matchesCombo(combo, event)) {
          event.preventDefault();
          handler(event);
          return;
        }
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [enabled]);
}

/** The shortcut list shown in the help overlay. */
export const SHORTCUTS: readonly (readonly [string, string])[] = [
  ["mod+d", "Switch IDE / Discord analysis"],
  ["mod+1 … mod+4", "Discord right panel: Analyze / Flag / Patterns / Ask"],
  ["mod+k", "Focus the message filter"],
  ["mod+enter", "Run the guardrails route for the selected user"],
  ["mod+e", "Save the current view as JSON"],
  ["mod+,", "Open settings / providers"],
  ["mod+/", "Toggle this help (or press ?)"],
];
