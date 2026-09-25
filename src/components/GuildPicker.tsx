import { useMemo, useRef, useState } from "react";
import type { DiscordGuildInfo } from "../ipc/contract";
import { readRecentGuilds } from "../lib/recents";

/**
 * Server navigation. An account can belong to a hundred guilds, so the list is
 * collapsed to the active server plus recents, and the full set is only listed
 * while a search is open.
 */
export default function GuildPicker({
  guilds,
  activeId,
  onSelect,
}: {
  guilds: DiscordGuildInfo[];
  activeId: string | null;
  onSelect: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);

  const active = guilds.find((g) => g.id === activeId) ?? null;
  const recents = useMemo(() => {
    const byId = new Map(guilds.map((g) => [g.id, g]));
    return readRecentGuilds()
      .map((id) => byId.get(id))
      .filter((g): g is DiscordGuildInfo => Boolean(g))
      .filter((g) => g.id !== activeId);
  }, [guilds, activeId]);

  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const pool = needle
      ? guilds.filter((g) => g.name.toLowerCase().includes(needle))
      : guilds;
    return [...pool].sort((a, b) => a.name.localeCompare(b.name));
  }, [guilds, query]);

  function choose(id: string) {
    setOpen(false);
    setQuery("");
    onSelect(id);
  }

  if (!open) {
    return (
      <div className="guild-picker">
        <button
          className="guild-current"
          onClick={() => {
            setOpen(true);
            // Focus after the input mounts.
            window.setTimeout(() => inputRef.current?.focus(), 0);
          }}
          title="Choose a server"
        >
          <span className="guild-current-name">{active ? active.name : "Choose a server…"}</span>
          <span className="guild-current-count">{guilds.length}</span>
        </button>
        {recents.length > 0 && (
          <div className="guild-recents">
            {recents.map((g) => (
              <button key={g.id} className="guild-chip" onClick={() => choose(g.id)} title={g.name}>
                {g.name}
              </button>
            ))}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="guild-picker">
      <input
        ref={inputRef}
        className="discord-search"
        placeholder={`Search ${guilds.length} servers…`}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            setOpen(false);
            setQuery("");
          } else if (e.key === "Enter" && matches.length > 0) {
            choose(matches[0].id);
          }
        }}
      />
      <ul className="discord-list guild-results">
        {matches.map((g) => (
          <li
            key={g.id}
            className={g.id === activeId ? "active" : ""}
            onClick={() => choose(g.id)}
          >
            {g.name}
          </li>
        ))}
        {matches.length === 0 && <li className="discord-hint">No server matches “{query}”.</li>}
      </ul>
    </div>
  );
}
