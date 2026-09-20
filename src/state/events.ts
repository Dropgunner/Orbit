import { listen } from "@tauri-apps/api/event";
import {
  AGENT_EVENT,
  TERMINAL_EVENT,
  type AgentEvent,
  type TerminalEvent,
} from "../ipc/contract";
import { useStore } from "./store";
import { unregisterTerminal, writeToTerminal } from "./terminals";

let initialized = false;

/// Subscribes to backend event channels and dispatches into the store /
/// terminal registry. Safe to call multiple times.
export async function initEvents(): Promise<void> {
  if (initialized) return;
  initialized = true;

  await listen<AgentEvent>(AGENT_EVENT, (event) => {
    const e = event.payload;
    const store = useStore.getState();
    switch (e.type) {
      case "delta":
        store.appendStreamingDelta(e.content);
        break;
      case "tool_call":
        store.appendToolCall(e.id, e.name, e.arguments);
        break;
      case "tool_result":
        store.appendToolResult(e.id, e.name, e.result);
        break;
      case "pending_edit":
        store.addPendingEdit(e.edit);
        break;
      case "done":
        store.finishStreaming(e.message);
        break;
      case "error":
        store.finishStreamingError(e.error);
        break;
    }
  });

  await listen<TerminalEvent>(TERMINAL_EVENT, (event) => {
    const e = event.payload;
    if (e.type === "data") {
      writeToTerminal(e.id, e.data);
    } else {
      useStore.getState().removeTerminal(e.id);
      unregisterTerminal(e.id);
    }
  });
}
