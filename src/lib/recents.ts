// Recently opened Discord servers, kept in localStorage next to the pane
// widths so the left column can lead with what the user actually uses instead
// of an alphabetical wall of every guild they belong to.

const RECENTS_KEY = "orbit.discord.recentGuilds";
const RECENTS_MAX = 5;

/** Server ids most recently opened, newest first. */
export function readRecentGuilds(): string[] {
  try {
    const raw = window.localStorage.getItem(RECENTS_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((id): id is string => typeof id === "string").slice(0, RECENTS_MAX);
  } catch {
    // A corrupt or unavailable store just means no recents.
    return [];
  }
}

/** Move `id` to the front of the recents list. */
export function rememberGuild(id: string): void {
  const next = [id, ...readRecentGuilds().filter((entry) => entry !== id)].slice(0, RECENTS_MAX);
  try {
    window.localStorage.setItem(RECENTS_KEY, JSON.stringify(next));
  } catch {
    /* storage unavailable; recents are a convenience, not state */
  }
}
