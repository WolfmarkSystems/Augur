import { useState } from "react";
import { useAppStore } from "../store/appStore";
import { isRtl } from "./languages";
import CodeSwitchBand from "./CodeSwitchBand";

export default function TranslationPanel() {
  const segments = useAppStore((s) => s.segments);
  const targetLang = useAppStore((s) => s.targetLang);
  const codeSwitches = useAppStore((s) => s.codeSwitches);
  const showBands = useAppStore((s) => s.showCodeSwitchBands);
  const isTranslating = useAppStore((s) => s.isTranslating);
  const flaggedSegments = useAppStore((s) => s.flaggedSegments);
  const flagSegment = useAppStore((s) => s.flagSegment);
  const unflagSegment = useAppStore((s) => s.unflagSegment);
  const [openPopover, setOpenPopover] = useState<number | null>(null);
  const [draftNote, setDraftNote] = useState<string>("");

  const dir = isRtl(targetLang.code) ? "rtl" : "ltr";
  const flaggedCount = Object.keys(flaggedSegments).length;

  return (
    <section className="panel panel-translation">
      <header className="panel-header">
        <span className="panel-title">Translation</span>
        <span className="panel-lang-badge">
          {targetLang.flag} {targetLang.name}
        </span>
        {isTranslating && (
          <span className="panel-pill panel-pill-teal">
            <span className="live-dot" /> live
          </span>
        )}
        {flaggedCount > 0 && (
          <span className="panel-pill panel-pill-danger">
            {flaggedCount} flagged
          </span>
        )}
      </header>
      <div className="panel-page" dir={dir}>
        {segments.length === 0 ? (
          <div className="panel-empty">
            Translation will appear here as AUGUR processes the file.
          </div>
        ) : (
          segments.map((seg, i) => {
            const csHere =
              showBands && codeSwitches.find((c) => c.offset === seg.index);
            const flag = flaggedSegments[seg.index];
            const isOpen = openPopover === seg.index;
            return (
              <div
                key={seg.index}
                className={`panel-row segment-row ${flag ? "is-flagged" : ""}`}
              >
                {csHere && (
                  <CodeSwitchBand
                    from={csHere.from}
                    to={csHere.to}
                    offset={csHere.offset}
                  />
                )}
                <div className="segment-row-body">
                  <p className="panel-text panel-text-translated">
                    {seg.translatedText}
                    {!seg.isComplete && i === segments.length - 1 && (
                      <span className="live-cursor" aria-hidden="true" />
                    )}
                  </p>
                  <button
                    type="button"
                    className={`flag-btn ${flag ? "is-active" : ""}`}
                    onClick={() => {
                      if (flag) {
                        unflagSegment(seg.index);
                        setOpenPopover(null);
                      } else {
                        setDraftNote("");
                        setOpenPopover(isOpen ? null : seg.index);
                      }
                    }}
                    title={flag ? "Unflag (currently flagged for review)" : "Flag for human review"}
                    aria-pressed={!!flag}
                  >
                    ⚑
                  </button>
                </div>
                {flag && flag.examinerNote && (
                  <div className="flag-note" title="Examiner note">
                    <strong>Note:</strong> {flag.examinerNote}
                  </div>
                )}
                {isOpen && !flag && (
                  <div className="flag-popover">
                    <textarea
                      autoFocus
                      placeholder="Note for reviewer (optional)…"
                      value={draftNote}
                      onChange={(e) => setDraftNote(e.target.value)}
                      rows={3}
                    />
                    <div className="flag-popover-actions">
                      <button
                        type="button"
                        className="btn"
                        onClick={() => setOpenPopover(null)}
                      >
                        Cancel
                      </button>
                      <button
                        type="button"
                        className="btn btn-primary"
                        onClick={() => {
                          flagSegment(seg.index, draftNote.trim());
                          setOpenPopover(null);
                        }}
                      >
                        Flag for Review
                      </button>
                    </div>
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}
