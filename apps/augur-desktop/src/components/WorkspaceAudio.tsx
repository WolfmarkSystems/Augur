import { useState } from "react";
import { useAppStore } from "../store/appStore";
import { isRtl } from "./languages";
import Waveform from "./Waveform";

function formatTs(ms?: number): string {
  if (ms == null) return "--:--";
  const s = Math.floor(ms / 1000);
  const mm = Math.floor(s / 60).toString().padStart(2, "0");
  const ss = (s % 60).toString().padStart(2, "0");
  return `${mm}:${ss}`;
}

/**
 * Sprint 12 P4 — transcript split view. Each segment row carries
 * a timestamp; clicking syncs both panels to the same row index.
 * Speaker headers ("─── SPEAKER_00 ───") render between rows
 * when the speaker changes.
 */
export default function WorkspaceAudio() {
  const segments = useAppStore((s) => s.segments);
  const sourceLang = useAppStore((s) => s.sourceLang);
  const targetLang = useAppStore((s) => s.targetLang);
  const isTranslating = useAppStore((s) => s.isTranslating);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);

  const dirSrc = isRtl(sourceLang.code) ? "rtl" : "ltr";
  const dirTgt = isRtl(targetLang.code) ? "rtl" : "ltr";

  let lastSpeaker: string | undefined;

  const playedRatio =
    segments.length === 0
      ? 0
      : segments.filter((s) => s.isComplete).length / segments.length;

  const renderColumn = (which: "source" | "target") => (
    <div
      className="audio-column"
      dir={which === "source" ? dirSrc : dirTgt}
    >
      {segments.length === 0 ? (
        <div className="panel-empty">
          {which === "source"
            ? "Audio transcript will appear here."
            : "Translated transcript will appear here."}
        </div>
      ) : (
        segments.map((seg, i) => {
          const speaker = seg.speakerId;
          const showSpeakerHeader =
            speaker && (i === 0 || speaker !== lastSpeaker);
          if (which === "source") lastSpeaker = speaker;
          const text = which === "source" ? seg.originalText : seg.translatedText;
          const live =
            isTranslating && !seg.isComplete && i === segments.length - 1;
          return (
            <div key={`${which}-${seg.index}`}>
              {showSpeakerHeader && (
                <div className="speaker-header">─── {speaker} ─────────────</div>
              )}
              <button
                type="button"
                className={`transcript-row ${activeIndex === i ? "is-active" : ""}`}
                onClick={() => setActiveIndex(i)}
              >
                <span className="ts">{formatTs(seg.startMs)}</span>
                <span className="tx">
                  {text}
                  {live && <span className="live-cursor" aria-hidden="true" />}
                </span>
              </button>
            </div>
          );
        })
      )}
    </div>
  );

  return (
    <div className="workspace workspace-audio">
      <div className="ws-pane">
        <section className="panel panel-source">
          <header className="panel-header">
            <span className="panel-title">Original</span>
            <span className="panel-lang-badge">
              {sourceLang.flag} {sourceLang.name}
            </span>
            <span className="panel-meta">transcript</span>
          </header>
          <div className="audio-waveform-wrap">
            <Waveform played={playedRatio} seed={31} />
          </div>
          <div className="audio-list">{renderColumn("source")}</div>
        </section>
      </div>
      <div className="ws-divider ws-divider-static" aria-hidden="true">
        <span className="ws-divider-grip">⋮⋮</span>
      </div>
      <div className="ws-pane">
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
          <div className="audio-waveform-wrap">
            <Waveform played={playedRatio} seed={97} />
          </div>
          <div className="audio-list">{renderColumn("target")}</div>
        </section>
      </div>
    </div>
  );
}
