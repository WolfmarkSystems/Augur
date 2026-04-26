import { useAppStore } from "../store/appStore";

const MT_ADVISORY_SHORT =
  "Machine translation — verify with human linguist";

function SelfTestSummary() {
  const fails = useAppStore((s) => s.selfTestFails);
  if (!fails || fails.length === 0) return null;
  return (
    <>
      <span className="status-sep" aria-hidden="true">|</span>
      <span
        className="statusbar-selftest"
        title={fails.join("\n")}
      >
        ● {fails.length} component{fails.length === 1 ? "" : "s"} unavailable
      </span>
    </>
  );
}

export default function StatusBar() {
  const isTranslating = useAppStore((s) => s.isTranslating);
  const activeEngine = useAppStore((s) => s.activeEngine);
  const sttModel = useAppStore((s) => s.sttModel);
  const dialect = useAppStore((s) => s.dialect);
  const segments = useAppStore((s) => s.segments);
  const errorMessage = useAppStore((s) => s.errorMessage);

  const leftPart = errorMessage ? (
    <span className="status-error">⚠ {errorMessage}</span>
  ) : isTranslating ? (
    <span className="status-active">
      <span className="status-dot" /> Translating ·{" "}
      {activeEngine ?? "Auto"}
    </span>
  ) : segments.length > 0 ? (
    <span className="status-idle">
      ✓ Done · {segments.length} segments
    </span>
  ) : (
    <span className="status-idle">Ready</span>
  );

  return (
    <footer className="statusbar" role="contentinfo">
      <span className="status-cell">{leftPart}</span>
      <span className="status-sep" aria-hidden="true">
        |
      </span>
      <span className="status-cell">
        {sttModel === "auto" ? "Whisper Auto" : sttModel} · offline
      </span>
      <span className="status-sep" aria-hidden="true">
        |
      </span>
      <span className="status-cell">
        {dialect
          ? `${dialect.dialect} ${dialect.confidence.toFixed(2)} (${dialect.source === "camel" ? "CAMeL" : "lexical"})`
          : "200 languages available"}
      </span>
      <span className="status-spacer" />
      <SelfTestSummary />
      <span
        className="status-mt-advisory"
        title="The machine-translation advisory cannot be dismissed."
        aria-live="polite"
      >
        ⚠ {MT_ADVISORY_SHORT}
      </span>
    </footer>
  );
}
