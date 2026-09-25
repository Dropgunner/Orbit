import { SHORTCUTS } from "../lib/hotkeys";

/** A modal listing the app's keyboard shortcuts. */
export default function ShortcutHelp({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  if (!open) return null;
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Keyboard shortcuts</h2>
        <table className="shortcut-table">
          <tbody>
            {SHORTCUTS.map(([combo, label]) => (
              <tr key={combo}>
                <td>
                  <kbd>{combo}</kbd>
                </td>
                <td>{label}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="modal-hint">
          <kbd>mod</kbd> is ⌘ on macOS and Ctrl elsewhere. Plain-key shortcuts are
          ignored while you are typing in a field.
        </p>
        <div className="modal-actions">
          <button className="primary" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
