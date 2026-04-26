import { useAppStore } from "../store/appStore";

function formatPath(path: string): string {
  // Show only the last 2 segments so the row stays compact.
  const parts = path.split("/");
  if (parts.length <= 2) return path;
  return `…/${parts[parts.length - 2]}/${parts[parts.length - 1]}`;
}

export default function WorkspaceBatch() {
  const batch = useAppStore((s) => s.batch);
  if (!batch) return null;

  const overallPct = batch.total > 0 ? Math.round((batch.processed / batch.total) * 100) : 0;

  return (
    <div className="batch-workspace">
      <header className="batch-header">
        <div>
          <div className="batch-title">Batch mode</div>
          <div className="batch-sub">{batch.inputDir}</div>
        </div>
        <div className="batch-counts">
          <span>{batch.total} files</span>
          <span>{batch.foreign} foreign</span>
          <span>{batch.translated} translated</span>
          {batch.errors > 0 && (
            <span className="batch-counts-err">{batch.errors} errors</span>
          )}
        </div>
      </header>

      <div className="batch-list">
        {batch.files.length === 0 ? (
          <div className="panel-empty">Discovering files…</div>
        ) : (
          batch.files.map((f, idx) => (
            <div key={f.path} className={`batch-row is-${f.status}`}>
              <span className="batch-row-idx">[{idx + 1}/{Math.max(batch.total, batch.files.length)}]</span>
              <span className="batch-row-name" title={f.path}>
                {formatPath(f.path)}
              </span>
              <span className="batch-row-meta">
                {f.detectedLanguage && <span className="batch-row-lang">{f.detectedLanguage}</span>}
                {f.inputType && <span className="batch-row-kind">{f.inputType}</span>}
              </span>
              <span className="batch-row-status">
                {f.status === "waiting" && <span className="waiting-pill">waiting</span>}
                {f.status === "active" && <span className="spinner" />}
                {f.status === "done" && (
                  <span className="check">{f.translated ? "✓ translated" : "✓"}</span>
                )}
                {f.status === "error" && (
                  <span className="batch-row-err" title={f.error ?? ""}>
                    ! error
                  </span>
                )}
              </span>
            </div>
          ))
        )}
      </div>

      <footer className="batch-footer">
        <div className="overall-progress">
          <div
            className="overall-progress-fill"
            style={{ width: `${overallPct}%` }}
          />
        </div>
        <div className="batch-footer-row">
          <span>
            Overall: {batch.processed}/{batch.total} complete · {batch.foreign} foreign found
          </span>
          <span>
            {batch.isRunning ? "Running…" : "Done"}
            {batch.outputPath && ` · report → ${formatPath(batch.outputPath)}`}
          </span>
        </div>
      </footer>
    </div>
  );
}
