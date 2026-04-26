import { useCallback, useRef, useState } from "react";
import SourcePanel from "./SourcePanel";
import TranslationPanel from "./TranslationPanel";

const MIN_PANEL = 280;

export default function WorkspaceDoc() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [leftWidthPct, setLeftWidthPct] = useState(50);
  const [dragging, setDragging] = useState(false);

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    setDragging(true);
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }, []);

  const onPointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!dragging || !containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      let pct = ((e.clientX - rect.left) / rect.width) * 100;
      const minPct = (MIN_PANEL / rect.width) * 100;
      const maxPct = 100 - minPct;
      pct = Math.max(minPct, Math.min(maxPct, pct));
      setLeftWidthPct(pct);
    },
    [dragging],
  );

  const onPointerUp = useCallback(() => {
    setDragging(false);
  }, []);

  return (
    <div
      ref={containerRef}
      className={`workspace workspace-doc ${dragging ? "is-dragging" : ""}`}
    >
      <div className="ws-pane" style={{ width: `${leftWidthPct}%` }}>
        <SourcePanel />
      </div>
      <div
        className="ws-divider"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize panels"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      >
        <span className="ws-divider-grip" aria-hidden="true">⋮⋮</span>
      </div>
      <div
        className="ws-pane"
        style={{ width: `calc(${100 - leftWidthPct}% - 8px)` }}
      >
        <TranslationPanel />
      </div>
    </div>
  );
}
