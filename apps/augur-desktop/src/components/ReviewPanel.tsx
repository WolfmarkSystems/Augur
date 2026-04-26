import { useState } from "react";
import { useAppStore } from "../store/appStore";

function formatTs(ms?: number): string {
  if (ms == null) return "--:--";
  const s = Math.floor(ms / 1000);
  const mm = Math.floor(s / 60).toString().padStart(2, "0");
  const ss = (s % 60).toString().padStart(2, "0");
  return `${mm}:${ss}`;
}

export default function ReviewPanel() {
  const segments = useAppStore((s) => s.segments);
  const flagged = useAppStore((s) => s.flaggedSegments);
  const setReviewStatus = useAppStore((s) => s.setReviewStatus);
  const unflagSegment = useAppStore((s) => s.unflagSegment);
  const [open, setOpen] = useState(false);

  const flaggedList = Object.values(flagged).sort(
    (a, b) => a.segmentIndex - b.segmentIndex,
  );

  const segmentByIndex = (i: number) =>
    segments.find((s) => s.index === i);

  if (flaggedList.length === 0 && !open) {
    return null;
  }

  return (
    <aside className={`review-panel ${open ? "is-open" : ""}`}>
      <button
        type="button"
        className="review-panel-tab"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        Needs Review {flaggedList.length > 0 && (
          <span className="review-panel-badge">{flaggedList.length}</span>
        )}
      </button>
      {open && (
        <div className="review-panel-body">
          {flaggedList.length === 0 ? (
            <div className="review-panel-empty">
              No segments flagged. Click the ⚑ icon next to any
              translated segment to flag it for review.
            </div>
          ) : (
            flaggedList.map((flag) => {
              const seg = segmentByIndex(flag.segmentIndex);
              return (
                <div
                  key={flag.segmentIndex}
                  className={`review-row review-row-${flag.reviewStatus}`}
                >
                  <div className="review-row-head">
                    <span className="review-ts">
                      {formatTs(seg?.startMs)}
                    </span>
                    <span className={`review-status-pill review-status-${flag.reviewStatus}`}>
                      {flag.reviewStatus.replace("_", " ")}
                    </span>
                  </div>
                  <div className="review-row-original">
                    {seg?.originalText ?? "(segment not loaded)"}
                  </div>
                  <div className="review-row-translation">
                    {seg?.translatedText ?? ""}
                  </div>
                  {flag.examinerNote && (
                    <div className="review-row-note">
                      <strong>Note:</strong> {flag.examinerNote}
                    </div>
                  )}
                  <div className="review-row-actions">
                    {flag.reviewStatus !== "reviewed" && (
                      <button
                        type="button"
                        className="btn"
                        onClick={() =>
                          setReviewStatus(flag.segmentIndex, "reviewed")
                        }
                      >
                        Mark Reviewed
                      </button>
                    )}
                    {flag.reviewStatus !== "disputed" && (
                      <button
                        type="button"
                        className="btn"
                        onClick={() =>
                          setReviewStatus(flag.segmentIndex, "disputed")
                        }
                      >
                        Mark Disputed
                      </button>
                    )}
                    <button
                      type="button"
                      className="btn"
                      onClick={() => unflagSegment(flag.segmentIndex)}
                    >
                      Remove Flag
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      )}
    </aside>
  );
}
