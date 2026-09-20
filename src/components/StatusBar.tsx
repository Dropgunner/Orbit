import { useStore } from "../state/store";

export default function StatusBar() {
  const statusMessage = useStore((s) => s.statusMessage);
  const gitStatus = useStore((s) => s.gitStatus);
  const streaming = useStore((s) => s.streaming);

  return (
    <footer className="statusbar">
      <span className="status-msg">{statusMessage}</span>
      <span className="status-spacer" />
      {streaming && <span className="status-streaming">● agent running</span>}
      {gitStatus && <span className="status-branch">⎇ {gitStatus.branch}</span>}
    </footer>
  );
}
