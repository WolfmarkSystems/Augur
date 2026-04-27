import { useMemo } from "react";
import { useAppStore } from "../store/appStore";
import { exportReport, saveReportDialog } from "../ipc";

interface Props {
  open: boolean;
  onClose: () => void;
}

function fmtDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const r = s % 60;
  return m > 0 ? `${m}m ${r}s` : `${r}s`;
}

export default function SaveLiveSessionDialog({ open, onClose }: Props) {
  const live = useAppStore((s) => s.live);
  const caseNumber = useAppStore((s) => s.caseNumber);
  const sourceLang = useAppStore((s) => s.sourceLang);
  const targetLang = useAppStore((s) => s.targetLang);
  const liveReset = useAppStore((s) => s.liveReset);
  const setError = useAppStore((s) => s.setError);

  const duration = useMemo(() => {
    if (!live.startedAt) return 0;
    const end = live.endedAt ?? new Date().toISOString();
    return new Date(end).getTime() - new Date(live.startedAt).getTime();
  }, [live.startedAt, live.endedAt]);

  if (!open || !live.info) return null;

  const segmentsForExport = live.segments.map((s) => ({
    index: s.chunkIndex,
    startMs: s.chunkStartMs,
    endMs: s.chunkEndMs,
    originalText: s.original,
    translatedText: s.translated,
    speakerId: null,
  }));

  const lastLang = live.segments.length
    ? live.segments[live.segments.length - 1].sourceLang
    : sourceLang.code;

  const handleExport = async (format: "html" | "json" | "zip") => {
    try {
      const out = await saveReportDialog({ format, caseNumber });
      if (!out) return;
      await exportReport({
        format,
        outputPath: out,
        caseNumber,
        sourceLang: lastLang,
        targetLang: live.info?.targetLanguage ?? targetLang.code,
        dialect: null,
        segments: segmentsForExport,
        flaggedSegments: [],
      });
      liveReset();
      onClose();
    } catch (e) {
      setError(`Live session export failed: ${String(e)}`);
    }
  };

  const handleDiscard = () => {
    liveReset();
    onClose();
  };

  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="overlay-backdrop" onClick={onClose} />
      <div className="overlay-panel">
        <header className="overlay-header">
          <span>Save live session?</span>
          <button
            type="button"
            className="overlay-close"
            onClick={onClose}
            aria-label="Close"
          >
            ×
          </button>
        </header>
        <div className="overlay-body">
          <p className="pkg-hint">The following was captured:</p>
          <ul className="live-summary">
            <li>Duration: <strong>{fmtDuration(duration)}</strong></li>
            <li>Detected language: <strong>{lastLang}</strong></li>
            <li>Segments: <strong>{live.segments.length}</strong></li>
            <li>Device: <code>{live.info.device}</code></li>
            <li>
              Translation target:{" "}
              <strong>{live.info.targetLanguage}</strong> · NLLB-200
            </li>
          </ul>
          <div className="mt-advisory">
            <strong>Forensic notice:</strong> Machine translation —
            verify with a certified human translator for legal proceedings.
            Live captures are even more advisory than offline runs.
          </div>
          <div className="pkg-actions">
            <button type="button" className="btn" onClick={handleDiscard}>
              Discard
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => handleExport("html")}
            >
              Save Transcript (HTML)
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => handleExport("zip")}
            >
              Save as Evidence Package (ZIP)
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
