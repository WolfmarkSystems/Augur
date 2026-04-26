interface Props {
  open: boolean;
  onClose: () => void;
}

export default function AboutDialog({ open, onClose }: Props) {
  if (!open) return null;
  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="overlay-backdrop" onClick={onClose} />
      <div className="overlay-panel about-dialog">
        <header className="overlay-header">
          <span>About AUGUR</span>
          <button
            type="button"
            className="overlay-close"
            onClick={onClose}
            aria-label="Close"
          >
            ×
          </button>
        </header>
        <div className="overlay-body about-body">
          <div className="about-mark" aria-hidden="true">A</div>
          <h2 className="about-title">AUGUR — Forensic Language Analysis</h2>
          <div className="about-version">Version 1.0.0</div>
          <div className="about-org">Wolfmark Systems</div>

          <p className="about-tagline">Built by operators, for operators.</p>

          <div className="about-section">
            <strong>Models:</strong> NLLB-200 (Meta AI), Whisper (OpenAI),
            SeamlessM4T (Meta AI), CAMeL Tools (Carnegie Mellon Univ.),
            fastText (Meta AI). All processing is performed locally.
            No evidence leaves your machine.
          </div>

          <div className="about-advisory">
            <strong>⚠ Machine Translation Notice</strong>
            <br />
            All translations produced by AUGUR are machine-generated and
            require verification by a certified human translator before
            use in legal proceedings.
          </div>

          <div className="about-copyright">© 2026 Wolfmark Systems</div>
        </div>
      </div>
    </div>
  );
}
