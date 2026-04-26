import { useEffect, useState } from "react";
import {
  getModelStatus,
  installModel,
  onModelInstallFinished,
  onModelInstallProgress,
  type ModelStatusResponse,
} from "../ipc";

interface Props {
  open: boolean;
  onClose: () => void;
}

interface PerModelInstall {
  inFlight: boolean;
  error?: string;
}

export default function ModelManager({ open, onClose }: Props) {
  const [status, setStatus] = useState<ModelStatusResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installs, setInstalls] = useState<Record<string, PerModelInstall>>({});

  const refresh = () => {
    getModelStatus()
      .then((s) => {
        setStatus(s);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  };

  useEffect(() => {
    if (!open) return;
    refresh();
    const unlisten: Array<() => void> = [];
    onModelInstallProgress((p) => {
      const id = p.model_id;
      if (p.type === "model_install_error") {
        setInstalls((prev) => ({
          ...prev,
          [id]: { inFlight: false, error: p.error ?? "install failed" },
        }));
      }
    }).then((u) => unlisten.push(u));
    onModelInstallFinished(({ model_id }) => {
      setInstalls((prev) => ({ ...prev, [model_id]: { inFlight: false } }));
      refresh();
    }).then((u) => unlisten.push(u));
    return () => unlisten.forEach((u) => u());
  }, [open]);

  const handleDownload = async (id: string) => {
    setInstalls((prev) => ({ ...prev, [id]: { inFlight: true } }));
    try {
      await installModel(id);
    } catch (e) {
      setInstalls((prev) => ({
        ...prev,
        [id]: { inFlight: false, error: String(e) },
      }));
    }
  };

  if (!open) return null;

  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="overlay-backdrop" onClick={onClose} />
      <div className="overlay-panel overlay-panel-wide">
        <header className="overlay-header">
          <span>Model Manager</span>
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
          {error ? (
            <div className="mm-error">
              ⚠ {error}
              <div className="mm-hint">
                The Model Manager needs the AUGUR CLI on PATH (or in
                <code> ~/.cargo/bin/</code>). Run the AUGUR Installer
                or <code>cargo install augur</code>.
              </div>
            </div>
          ) : !status ? (
            <div className="mm-hint">Loading installed-model status…</div>
          ) : (
            <>
              <div className="mm-profile-row">
                <span className="mm-profile-label">Detected profile:</span>
                <span className={`mm-profile-tag mm-profile-${status.profile}`}>
                  {status.profile}
                </span>
                <span className="mm-row-size">
                  {(status.total_installed_bytes / 1e9).toFixed(2)} GB /{" "}
                  {(status.total_bytes / 1e9).toFixed(2)} GB
                </span>
              </div>
              <ul className="mm-list">
                {status.models.map((m) => {
                  const inst = installs[m.id];
                  return (
                    <li
                      key={m.id}
                      className={`mm-row ${m.installed ? "is-installed" : ""}`}
                    >
                      <span className="mm-row-name">{m.name}</span>
                      <span className="mm-row-tier">{m.tier}</span>
                      <span className="mm-row-size">{m.size_display}</span>
                      <span className="mm-row-status-cell">
                        {m.installed ? (
                          <span className="mm-row-status ok">✓ installed</span>
                        ) : inst?.inFlight ? (
                          <span className="mm-row-status">
                            <span className="spinner" /> downloading…
                          </span>
                        ) : (
                          <button
                            type="button"
                            className="btn-mm-download"
                            onClick={() => handleDownload(m.id)}
                          >
                            Download
                          </button>
                        )}
                      </span>
                      {inst?.error && (
                        <span className="mm-row-err" title={inst.error}>
                          ⚠ {inst.error.slice(0, 60)}
                        </span>
                      )}
                    </li>
                  );
                })}
              </ul>
              <p className="mm-hint">
                Models live under{" "}
                <code>~/.cache/augur/models/</code>. The AUGUR Installer is the
                bulk-install path; this panel installs single models for
                ad-hoc additions.
              </p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
