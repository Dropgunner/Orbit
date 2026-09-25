/** Pane-layout helpers shared by the IDE and Discord workspaces. */

/** Clamp a pane width to `[min, max]`, tolerating a `max` below `min`. */
export function clampWidth(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

/** Read a persisted pane width from localStorage, falling back when absent. */
export function readStoredWidth(key: string, fallback: number): number {
  const raw = window.localStorage.getItem(key);
  const parsed = raw ? Number(raw) : NaN;
  return Number.isFinite(parsed) ? parsed : fallback;
}
