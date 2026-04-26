import { useMemo } from "react";

interface Props {
  bars?: number;
  /** 0..1 fraction shown as the played portion. */
  played?: number;
  seed?: number;
}

/**
 * Simple amplitude visualisation. Sprint 12 P4 — does not need
 * to be interactive; we draw a deterministic-pseudo-random
 * waveform so the layout is stable across renders without
 * pulling a real Web Audio decode for the placeholder pipeline.
 */
export default function Waveform({ bars = 90, played = 0, seed = 17 }: Props) {
  const heights = useMemo(() => {
    const out: number[] = [];
    let x = seed;
    for (let i = 0; i < bars; i++) {
      // xorshift32 — deterministic, no deps
      x ^= x << 13;
      x ^= x >>> 17;
      x ^= x << 5;
      x = x | 0;
      const v = ((x >>> 0) % 100) / 100;
      // bias toward middle amplitude
      out.push(0.3 + v * 0.7);
    }
    return out;
  }, [bars, seed]);

  const cutoff = Math.floor(played * bars);

  return (
    <div className="waveform" aria-hidden="true">
      {heights.map((h, i) => (
        <span
          key={i}
          className={`wf-bar ${i < cutoff ? "wf-bar-played" : ""}`}
          style={{ height: `${Math.round(h * 100)}%` }}
        />
      ))}
    </div>
  );
}
