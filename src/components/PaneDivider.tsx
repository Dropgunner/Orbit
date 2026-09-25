import type { PointerEvent as ReactPointerEvent } from "react";

/**
 * A draggable divider between two panes. Reports the cumulative pointer delta
 * since the drag started via `onDelta`; the caller captures the pane's width at
 * `onStart` and applies the delta. Double-click calls `onReset`.
 */
export default function PaneDivider({
  orientation = "vertical",
  onStart,
  onDelta,
  onReset,
  title = "Drag to resize · double-click to reset",
}: {
  orientation?: "vertical" | "horizontal";
  onStart?: () => void;
  onDelta: (delta: number) => void;
  onReset?: () => void;
  title?: string;
}) {
  function onPointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const start = orientation === "vertical" ? event.clientX : event.clientY;
    onStart?.();
    const move = (ev: PointerEvent) => {
      const current = orientation === "vertical" ? ev.clientX : ev.clientY;
      onDelta(current - start);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  return (
    <div
      className={`pane-divider pane-divider-${orientation}`}
      onPointerDown={onPointerDown}
      onDoubleClick={onReset}
      title={title}
      role="separator"
      aria-orientation={orientation}
    />
  );
}
