import { useAppStore } from "../store/appStore";
import { isRtl } from "./languages";
import CodeSwitchBand from "./CodeSwitchBand";

export default function TranslationPanel() {
  const segments = useAppStore((s) => s.segments);
  const targetLang = useAppStore((s) => s.targetLang);
  const codeSwitches = useAppStore((s) => s.codeSwitches);
  const showBands = useAppStore((s) => s.showCodeSwitchBands);
  const isTranslating = useAppStore((s) => s.isTranslating);

  const dir = isRtl(targetLang.code) ? "rtl" : "ltr";

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
      </header>
      <div className="panel-page" dir={dir}>
        {segments.length === 0 ? (
          <div className="panel-empty">
            Translation will appear here as AUGUR processes the file.
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
                <p className="panel-text panel-text-translated">
                  {seg.translatedText}
                  {!seg.isComplete && i === segments.length - 1 && (
                    <span className="live-cursor" aria-hidden="true" />
                  )}
                </p>
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}
