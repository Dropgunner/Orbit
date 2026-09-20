import { useEffect, useRef, useState } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { ipc } from "../ipc/invoke";
import { useStore } from "../state/store";
import {
  getTerminal,
  registerTerminal,
  unregisterTerminal,
} from "../state/terminals";

function TerminalInstance({ id, active }: { id: string; active: boolean }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const term = new XTerm({
      fontFamily: "Menlo, Monaco, monospace",
      fontSize: 12,
      cursorBlink: true,
      allowProposedApi: true,
      theme: {
        background: "#0d1117",
        foreground: "#d1d5db",
        cursor: "#6c8cff",
        selectionBackground: "#2a3a5c",
        black: "#1f2428",
        red: "#f47067",
        green: "#57ab5a",
        yellow: "#c69026",
        blue: "#539bf5",
        magenta: "#b083f0",
        cyan: "#39c5cf",
        white: "#d1d5db",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    try {
      fit.fit();
    } catch {
      /* ignore */
    }
    registerTerminal(id, { term, fit });

    const dataSub = term.onData((data) => {
      void ipc.writeTerminal(id, data);
    });

    const resize = () => {
      try {
        fit.fit();
        if (term.cols > 0 && term.rows > 0) {
          void ipc.resizeTerminal(id, term.cols, term.rows);
        }
      } catch {
        /* ignore */
      }
    };
    const observer = new ResizeObserver(resize);
    observer.observe(el);
    const timer = window.setTimeout(resize, 120);

    return () => {
      window.clearTimeout(timer);
      observer.disconnect();
      dataSub.dispose();
      unregisterTerminal(id);
      void ipc.killTerminal(id);
    };
  }, [id]);

  useEffect(() => {
    if (active) {
      const handle = getTerminal(id);
      if (handle) {
        try {
          handle.fit.fit();
        } catch {
          /* ignore */
        }
      }
    }
  }, [active, id]);

  return <div ref={ref} className={"terminal-instance" + (active ? " active" : "")} />;
}

export default function Terminal() {
  const terminalIds = useStore((s) => s.terminalIds);
  const activeTerminal = useStore((s) => s.activeTerminal);
  const setActiveTerminal = useStore((s) => s.setActiveTerminal);
  const addTerminal = useStore((s) => s.addTerminal);
  const projectRoot = useStore((s) => s.projectRoot);
  const [open, setOpen] = useState(true);

  async function newTerminal() {
    const id = await ipc.spawnTerminal(80, 24, projectRoot);
    addTerminal(id);
  }

  return (
    <div className="terminal">
      <div className="terminal-header">
        <button className="terminal-toggle" onClick={() => setOpen(!open)}>
          {open ? "▾" : "▸"}
        </button>
        <span className="terminal-title">Terminal</span>
        {terminalIds.map((id) => (
          <button
            key={id}
            className={"terminal-tab" + (id === activeTerminal ? " active" : "")}
            onClick={() => setActiveTerminal(id)}
          >
            sh
          </button>
        ))}
        <button className="terminal-add" onClick={() => void newTerminal()}>
          +
        </button>
        <button
          className="terminal-kill"
          title="Kill terminal"
          onClick={() => {
            if (activeTerminal) void ipc.killTerminal(activeTerminal);
          }}
        >
          ⏻
        </button>
      </div>
      {open && (
        <div className="terminal-body">
          {terminalIds.length === 0 ? (
            <div className="terminal-empty">No terminal — open a folder or press +</div>
          ) : (
            terminalIds.map((id) => (
              <TerminalInstance key={id} id={id} active={id === activeTerminal} />
            ))
          )}
        </div>
      )}
    </div>
  );
}
