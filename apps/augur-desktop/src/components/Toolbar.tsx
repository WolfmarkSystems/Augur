import { useAppStore, type SttModel, type TranslationEngine } from "../store/appStore";
import LangPicker from "./LangPicker";

const STT_OPTIONS: { value: SttModel; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "whisper-tiny", label: "Whisper Tiny" },
  { value: "whisper-base", label: "Whisper Base" },
  { value: "whisper-large-v3", label: "Whisper Large-v3" },
  { value: "whisper-pashto", label: "Whisper Pashto" },
  { value: "whisper-dari", label: "Whisper Dari" },
];

const ENGINE_OPTIONS: { value: TranslationEngine; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "nllb-600m", label: "NLLB-200 600M" },
  { value: "nllb-1b3", label: "NLLB-200 1.3B" },
  { value: "seamless-m4t", label: "SeamlessM4T" },
];

const ENGINE_LABEL: Record<TranslationEngine, string> = {
  auto: "Auto",
  "nllb-600m": "NLLB-200 600M",
  "nllb-1b3": "NLLB-200 1.3B",
  "seamless-m4t": "SeamlessM4T",
};

export default function Toolbar() {
  const sourceLang = useAppStore((s) => s.sourceLang);
  const targetLang = useAppStore((s) => s.targetLang);
  const setSourceLang = useAppStore((s) => s.setSourceLang);
  const setTargetLang = useAppStore((s) => s.setTargetLang);
  const sttModel = useAppStore((s) => s.sttModel);
  const setSttModel = useAppStore((s) => s.setSttModel);
  const engine = useAppStore((s) => s.translationEngine);
  const setEngine = useAppStore((s) => s.setTranslationEngine);
  const activeEngine = useAppStore((s) => s.activeEngine);
  const isTranslating = useAppStore((s) => s.isTranslating);
  const dialect = useAppStore((s) => s.dialect);

  const sourceTrailingNote = dialect
    ? `${dialect.dialect.split(" ")[0]} dialect`
    : undefined;

  return (
    <div className="toolbar" role="toolbar">
      <div className="toolbar-lang-group">
        <LangPicker
          selected={sourceLang}
          role="source"
          trailingNote={sourceTrailingNote}
          onSelect={setSourceLang}
        />
        <span className="toolbar-arrow" aria-hidden="true">
          →
        </span>
        <LangPicker
          selected={targetLang}
          role="target"
          onSelect={setTargetLang}
        />
      </div>
      <div className="toolbar-divider" aria-hidden="true" />
      <select
        className="toolbar-select"
        value={sttModel}
        onChange={(e) => setSttModel(e.target.value as SttModel)}
        aria-label="STT model"
      >
        {STT_OPTIONS.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <select
        className="toolbar-select"
        value={engine}
        onChange={(e) => setEngine(e.target.value as TranslationEngine)}
        aria-label="Translation engine"
      >
        {ENGINE_OPTIONS.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      {activeEngine && (
        <span className="engine-badge" title={`Active engine: ${ENGINE_LABEL[activeEngine]}`}>
          {ENGINE_LABEL[activeEngine]}
        </span>
      )}
      <div className="toolbar-spacer" />
      <button
        type="button"
        className={`live-btn ${isTranslating ? "is-active" : ""}`}
        aria-pressed={isTranslating}
        disabled
      >
        <span className="live-btn-dot" aria-hidden="true" />
        {isTranslating ? "Live" : "Idle"}
      </button>
    </div>
  );
}
