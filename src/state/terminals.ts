import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";

export interface TermHandle {
  term: Terminal;
  fit: FitAddon;
}

const registry = new Map<string, TermHandle>();

// Output that arrives before the terminal is mounted (e.g. the initial shell
// prompt) is buffered here and flushed on registration so it is not lost.
const pending = new Map<string, string[]>();

export function registerTerminal(id: string, handle: TermHandle): void {
  registry.set(id, handle);
  const queued = pending.get(id);
  if (queued && queued.length > 0) {
    for (const chunk of queued) handle.term.write(chunk);
  }
  pending.delete(id);
}

export function getTerminal(id: string): TermHandle | undefined {
  return registry.get(id);
}

export function writeToTerminal(id: string, data: string): void {
  const handle = registry.get(id);
  if (handle) {
    handle.term.write(data);
    return;
  }
  const queued = pending.get(id) ?? [];
  queued.push(data);
  pending.set(id, queued);
}

export function unregisterTerminal(id: string): void {
  const handle = registry.get(id);
  handle?.term.dispose();
  registry.delete(id);
  pending.delete(id);
}

export function disposeAllTerminals(): void {
  for (const id of [...registry.keys()]) {
    unregisterTerminal(id);
  }
}
