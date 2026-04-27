import { useEffect, useRef } from "react";
import { useAppStore } from "../store/appStore";
import { stopLiveTranslation } from "../ipc";

const LIVE_ADVISORY =
  "LIVE MACHINE TRANSLATION — NOT VERIFIED. Real-time translation is " +
  "inherently less accurate than offline processing. Do not use for legal " +
  "decisions in real time.";

function formatTs(ms: number): string {
  const s = Math.floor(ms / 1000);
  const mm = Math.floor(s / 60).toString().padStart(2, "0");
  const ss = (s % 60).toString().padStart(2, "0");
  return `${mm}:${ss}`;
}

interface Props {
  onSaveSession: () => void;
}

export default function WorkspaceLive({ onSaveSession }: Props) {
  const live = useAppStore((s) => s.live);
  const liveStop = useAppStore((s) => s.liveStop);
  const sourceListRef = useRef<HTMLDivElement | null>(null);
  const targetListRef = useRef<HTMLDivElement | null>(null);

  // Auto-scroll both panels to the bottom as new segments land.
  useEffect(() => {
    sourceListRef.current?.scrollTo({
      top: sourceListRef.current.scrollHeight,
    });
    targetListRef.current?.scrollTo({
      top: targetListRef.current.scrollHeight,
    });
  }, [live.segments.length]);

  const handleStop = async () => {
    try {
      await stopLiveTranslation();
    } catch (e) {
      // Stop is best-effort — even if the IPC errors the
      // background event handler will mark us as stopped.
      void e;
    }
    liveStop();
    if (live.segments.length > 0) {
      onSaveSession();
    }
  };

  const duration = live.startedAt
    ? Math.max(
        0,
        Math.floor(
          (Date.now() - new Date(live.startedAt).getTime()) / 1000,
        ),
      )
    : 0;

  const lastLang = live.segments.length
    ? live.segments[live.segments.length - 1].sourceLang
    : "—";

  return (
    <div className="workspace workspace-live">
      <div className="live-banner" role="alert">
        <strong>⚠ LIVE MACHINE TRANSLATION — NOT VERIFIED.</strong>{" "}
        {LIVE_ADVISORY}
      </div>
      <div className="live-toolbar">
        <span className="live-rec">
          <span className="live-rec-dot" />{" "}
          {live.isRecording ? "RECORDING" : "STOPPED"}
        </span>
        <span className="live-meta">
          Source auto-detect → <strong>{live.info?.targetLanguage ?? "en"}</strong>
        </span>
        <span className="live-meta">
          Device: {live.info?.device ?? "—"}
        </span>
        <span className="live-spacer" />
        <button
          type="button"
          className="btn btn-primary live-stop-btn"
          onClick={handleStop}
          disabled={!live.isRecording}
        >
          ■ Stop
        </button>
      </div>
      {live.errorMessage && (
        <div className="pkg-error">⚠ {live.errorMessage}</div>
      )}
      <div className="live-split">
        <section className="live-pane">
          <header className="panel-header">
            <span className="panel-title">Original</span>
          </header>
          <div className="live-list" ref={sourceListRef}>
            {live.segments.length === 0 ? (
              <div className="panel-empty">
                Waiting for the first chunk… (Whisper model may be
                downloading on the first launch.)
              </div>
            ) : (
              live.segments.map((seg) => (
                <div key={seg.chunkIndex} className="live-row">
                  <span className="live-ts">{formatTs(seg.chunkStartMs)}</span>
                  <span className="live-text">{seg.original}</span>
                </div>
              ))
            )}
            {live.isRecording && (
              <div className="live-row">
                <span className="live-ts">{formatTs(duration * 1000)}</span>
                <span className="live-text">
                  <span className="live-cursor" aria-hidden="true" />
                </span>
              </div>
            )}
          </div>
        </section>
        <div className="ws-divider ws-divider-static" aria-hidden="true">
          <span className="ws-divider-grip">⋮⋮</span>
        </div>
        <section className="live-pane">
          <header className="panel-header">
            <span className="panel-title">Translation</span>
          </header>
          <div className="live-list" ref={targetListRef}>
            {live.segments.length === 0 ? (
              <div className="panel-empty">
                Translations stream here as chunks complete.
              </div>
            ) : (
              live.segments.map((seg) => (
                <div key={seg.chunkIndex} className="live-row">
                  <span className="live-ts">{formatTs(seg.chunkStartMs)}</span>
                  <span className="live-text live-text-translated">
                    {seg.translated}
                  </span>
                </div>
              ))
            )}
          </div>
        </section>
      </div>
      <footer className="live-footer">
        <span>Segments: {live.segments.length}</span>
        <span>Duration: {formatTs(duration * 1000)}</span>
        <span>Language: {lastLang}</span>
        <span className="live-footer-advisory">⚠ LIVE MT — UNVERIFIED</span>
      </footer>
    </div>
  );
}
