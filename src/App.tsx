import { useEffect, useRef, useState } from "react";
import { initEvents } from "./state/events";
import { useStore } from "./state/store";
import { useDiscordStore } from "./state/discordStore";
import { clampWidth, readStoredWidth } from "./lib/pane";
import { useHotkeys } from "./lib/hotkeys";
import { isTauri } from "./lib/tauri";
import Header from "./components/Header";
import Sidebar from "./components/Sidebar";
import EditorPane from "./components/EditorPane";
import Terminal from "./components/Terminal";
import AgentPane from "./components/AgentPane";
import StatusBar from "./components/StatusBar";
import SettingsModal from "./components/SettingsModal";
import ShortcutHelp from "./components/ShortcutHelp";
import DiscordPanel from "./components/DiscordPanel";
import PaneDivider from "./components/PaneDivider";

const SIDEBAR_MIN = 180;
const SIDEBAR_DEFAULT = 260;
const CHAT_MIN = 260;
const CHAT_DEFAULT = 380;
const CENTER_MIN = 320;
const TERMINAL_MIN = 80;
const TERMINAL_DEFAULT = 220;

export default function App() {
  const loadSettings = useStore((s) => s.loadSettings);
  const setSettingsOpen = useStore((s) => s.setSettingsOpen);
  const discordMode = useDiscordStore((s) => s.discordMode);
  const setDiscordMode = useDiscordStore((s) => s.setDiscordMode);
  const loadGuardPolicies = useDiscordStore((s) => s.loadGuardPolicies);
  const loadPatterns = useDiscordStore((s) => s.loadPatterns);
  const loadFlags = useDiscordStore((s) => s.loadFlags);
  const [helpOpen, setHelpOpen] = useState(false);

  useHotkeys({
    "mod+d": () => setDiscordMode(!discordMode),
    "mod+,": () => setSettingsOpen(true),
    "mod+/": () => setHelpOpen((open) => !open),
    "?": () => setHelpOpen((open) => !open),
    escape: () => setHelpOpen(false),
  });

  const [sidebarWidth, setSidebarWidth] = useState(() =>
    readStoredWidth("orbit.ide.sidebarWidth", SIDEBAR_DEFAULT)
  );
  const [chatWidth, setChatWidth] = useState(() =>
    readStoredWidth("orbit.ide.chatWidth", CHAT_DEFAULT)
  );
  const [terminalHeight, setTerminalHeight] = useState(() =>
    readStoredWidth("orbit.ide.terminalHeight", TERMINAL_DEFAULT)
  );
  const sidebarBase = useRef(SIDEBAR_DEFAULT);
  const chatBase = useRef(CHAT_DEFAULT);
  const terminalBase = useRef(TERMINAL_DEFAULT);

  useEffect(() => {
    void initEvents();
    void loadSettings();
    void loadGuardPolicies();
    void loadPatterns();
    void loadFlags();
    // The local web UI is Discord-only: the IDE needs a filesystem and a PTY.
    if (!isTauri()) setDiscordMode(true);
  }, [loadSettings, loadGuardPolicies, loadPatterns, loadFlags, setDiscordMode]);

  // Persist IDE pane widths so the layout is stable across restarts.
  useEffect(() => {
    window.localStorage.setItem("orbit.ide.sidebarWidth", String(sidebarWidth));
  }, [sidebarWidth]);
  useEffect(() => {
    window.localStorage.setItem("orbit.ide.chatWidth", String(chatWidth));
  }, [chatWidth]);
  useEffect(() => {
    window.localStorage.setItem("orbit.ide.terminalHeight", String(terminalHeight));
  }, [terminalHeight]);

  // Keep the panes inside the window when it is resized.
  useEffect(() => {
    function onResize() {
      const total = window.innerWidth;
      setSidebarWidth((w) => clampWidth(w, SIDEBAR_MIN, total - CHAT_MIN - CENTER_MIN));
      setChatWidth((w) => clampWidth(w, CHAT_MIN, total - SIDEBAR_MIN - CENTER_MIN));
      setTerminalHeight((h) =>
        clampWidth(h, TERMINAL_MIN, window.innerHeight - TERMINAL_MIN - 160)
      );
    }
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return (
    <div className="app">
      <Header />
      {discordMode || !isTauri() ? (
        <div className="main main-discord">
          <DiscordPanel />
        </div>
      ) : (
        <div className="main main-ide">
          <div className="pane-fixed" style={{ width: sidebarWidth }}>
            <Sidebar />
          </div>
          <PaneDivider
            onStart={() => {
              sidebarBase.current = sidebarWidth;
            }}
            onDelta={(delta) =>
              setSidebarWidth(
                clampWidth(
                  sidebarBase.current + delta,
                  SIDEBAR_MIN,
                  window.innerWidth - chatWidth - CENTER_MIN
                )
              )
            }
            onReset={() => setSidebarWidth(SIDEBAR_DEFAULT)}
          />
          <div className="center">
            <EditorPane />
            <PaneDivider
              orientation="horizontal"
              onStart={() => {
                terminalBase.current = terminalHeight;
              }}
              onDelta={(delta) =>
                setTerminalHeight(
                  clampWidth(
                    terminalBase.current - delta,
                    TERMINAL_MIN,
                    window.innerHeight - TERMINAL_MIN - 160
                  )
                )
              }
              onReset={() => setTerminalHeight(TERMINAL_DEFAULT)}
            />
            <div className="terminal-wrap" style={{ height: terminalHeight }}>
              <Terminal />
            </div>
          </div>
          <PaneDivider
            onStart={() => {
              chatBase.current = chatWidth;
            }}
            onDelta={(delta) =>
              setChatWidth(
                clampWidth(
                  chatBase.current - delta,
                  CHAT_MIN,
                  window.innerWidth - sidebarWidth - CENTER_MIN
                )
              )
            }
            onReset={() => setChatWidth(CHAT_DEFAULT)}
          />
          <div className="pane-fixed" style={{ width: chatWidth }}>
            <AgentPane />
          </div>
        </div>
      )}
      <StatusBar />
      <SettingsModal />
      <ShortcutHelp open={helpOpen} onClose={() => setHelpOpen(false)} />
    </div>
  );
}
