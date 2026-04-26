import { useAppStore } from "../store/appStore";
import { isRtl } from "./languages";
import CodeSwitchBand from "./CodeSwitchBand";
import DialectCard from "./DialectCard";

export default function SourcePanel() {
  const segments = useAppStore((s) => s.segments);
  const sourceLang = useAppStore((s) => s.sourceLang);
  const dialect = useAppStore((s) => s.dialect);
  const codeSwitches = useAppStore((s) => s.codeSwitches);
  const showDialect = useAppStore((s) => s.showDialectCard);
  const showBands = useAppStore((s) => s.showCodeSwitchBands);
  const hasCs = useAppStore((s) => s.hasCodeSwitching);

  const wordCount = segments.reduce(
    (acc, s) => acc + s.originalText.split(/\s+/).filter(Boolean).length,
    0,
  );

  const dir = isRtl(sourceLang.code) ? "rtl" : "ltr";

  return (
    <section className="panel panel-source">
      <header className="panel-header">
        <span className="panel-title">Original</span>
        <span className="panel-lang-badge">
          {sourceLang.flag} {sourceLang.name}
        </span>
        <span className="panel-meta">{wordCount} words</span>
        {hasCs && (
          <span className="panel-pill panel-pill-amber">
            code-switching
          </span>
        )}
      </header>
      <div className="panel-page" dir={dir}>
        {segments.length === 0 ? (
          <div className="panel-empty">
            Open evidence with <strong>File → Open Evidence…</strong> to start.
          </div>
        ) : (
          segments.map((seg, i) => {
            const csHere =
              showBands &&
              codeSwitches.find((c) => c.offset === seg.index);
            return (
              <div key={seg.index} className="panel-row">
                {csHere && (
                  <CodeSwitchBand
                    from={csHere.from}
                    to={csHere.to}
                    offset={csHere.offset}
                  />
                )}
                <p className="panel-text">{seg.originalText}</p>
                {!seg.isComplete && i === segments.length - 1 && (
                  <span className="live-cursor" aria-hidden="true" />
                )}
              </div>
            );
          })
        )}
        {showDialect && dialect && <DialectCard dialect={dialect} />}
      </div>
    </section>
  );
}
