interface Props {
  from: string;
  to: string;
  offset: number;
}

export default function CodeSwitchBand({ from, to, offset }: Props) {
  return (
    <div className="switch-band" aria-label="language code-switch">
      <div className="switch-dot" aria-hidden="true" />
      <span className="switch-text">
        language switch: <strong>{from}</strong> → <strong>{to}</strong>{" "}
        <span className="switch-offset">(offset {offset})</span>
      </span>
    </div>
  );
}
