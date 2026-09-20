import { useEffect } from "react";
import { initEvents } from "./state/events";
import { useStore } from "./state/store";
import Header from "./components/Header";
import Sidebar from "./components/Sidebar";
import EditorPane from "./components/EditorPane";
import Terminal from "./components/Terminal";
import ChatPanel from "./components/ChatPanel";
import StatusBar from "./components/StatusBar";
import SettingsModal from "./components/SettingsModal";

export default function App() {
  const loadSettings = useStore((s) => s.loadSettings);

  useEffect(() => {
    void initEvents();
    void loadSettings();
  }, [loadSettings]);

  return (
    <div className="app">
      <Header />
      <div className="main">
        <Sidebar />
        <div className="center">
          <EditorPane />
          <Terminal />
        </div>
        <ChatPanel />
      </div>
      <StatusBar />
      <SettingsModal />
    </div>
  );
}
