import type { DialectInfo } from "../types";

interface Props {
  dialect: DialectInfo;
}

export default function DialectCard({ dialect }: Props) {
  const pct = Math.round(dialect.confidence * 100);
  const sourceLabel =
    dialect.source === "camel"
      ? "CAMeL Tools · Carnegie Mellon"
      : "Script analysis · lexical markers";
  return (
    <aside className="dialect-card" aria-label="Detected dialect">
      <div className="dc-row">
        <span className="dc-label">Dialect</span>
        <span className="dc-val">{dialect.dialect}</span>
      </div>
      <div className="dc-row">
        <span className="dc-label">Confidence</span>
        <span className="dc-val">{dialect.confidence.toFixed(2)}</span>
      </div>
      <div className="dc-bar" aria-hidden="true">
        <div className="dc-fill" style={{ width: `${pct}%` }} />
      </div>
      <div className="dc-src">{sourceLabel}</div>
      {dialect.indicators && dialect.indicators.length > 0 && (
        <div className="dc-indicators">
          {dialect.indicators.slice(0, 6).map((w) => (
            <span key={w} className="dc-pill">
              {w}
            </span>
          ))}
        </div>
      )}
      <div className="dc-advisory">
        Verify dialect with a human linguist before relying on this label
        in legal proceedings.
      </div>
    </aside>
  );
}
