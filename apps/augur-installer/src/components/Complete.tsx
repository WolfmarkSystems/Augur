import { launchAugur } from "../ipc";
import type { InstallResult } from "../types";

const LANGUAGES_FOR: Record<InstallResult["profile"], number> = {
  minimal: 99,
  standard: 200,
  full: 200,
};

const MT_ADVISORY =
  "Machine translation — verify with a certified human translator for legal proceedings.";

export default function Complete({ result }: { result: InstallResult }) {
  const sizeGb = (result.totalBytes / 1e9).toFixed(1);
  const langs = LANGUAGES_FOR[result.profile] ?? 200;
  return (
    <div className="complete-screen">
      <div className="complete-icon" aria-hidden="true">
        ✓
      </div>
      <h2 className="screen-title">AUGUR is ready</h2>
      <p className="screen-sub">
        Your {result.profile} install is complete.
      </p>
      <div className="stat-cards">
        <div className="stat-card">
          <div className="stat-value">{result.componentCount}</div>
          <div className="stat-label">Components installed</div>
        </div>
        <div className="stat-card">
          <div className="stat-value">{langs}</div>
          <div className="stat-label">Languages supported</div>
        </div>
        <div className="stat-card">
          <div className="stat-value">{sizeGb} GB</div>
          <div className="stat-label">Total size</div>
        </div>
      </div>
      <div className="mt-advisory">
        <strong>Forensic notice:</strong> {MT_ADVISORY}
      </div>
      <button
        type="button"
        className="btn btn-primary btn-launch"
        onClick={() => {
          launchAugur().catch(() => {
            // Launching might fail if the main AUGUR.app isn't
            // installed yet — show the path the user can run
            // from a terminal instead.
          });
        }}
      >
        Launch AUGUR
      </button>
    </div>
  );
}
