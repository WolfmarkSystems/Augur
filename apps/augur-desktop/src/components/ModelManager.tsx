import { useEffect, useState } from "react";
import { detectedProfile, listModels } from "../ipc";
import type { ModelStatus } from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function ModelManager({ open, onClose }: Props) {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [profile, setProfile] = useState<string>("none");

  useEffect(() => {
    if (!open) return;
    listModels().then(setModels).catch(() => setModels([]));
    detectedProfile().then(setProfile).catch(() => setProfile("none"));
  }, [open]);

  if (!open) return null;

  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="overlay-backdrop" onClick={onClose} />
      <div className="overlay-panel">
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
          <div className="mm-profile-row">
            <span className="mm-profile-label">Detected profile:</span>
            <span className={`mm-profile-tag mm-profile-${profile}`}>
              {profile}
            </span>
          </div>
          <ul className="mm-list">
            {models.map((m) => (
              <li
                key={m.id}
                className={`mm-row ${m.installed ? "is-installed" : ""}`}
              >
                <span className="mm-row-name">{m.name}</span>
                <span className="mm-row-tier">{m.tier}</span>
                <span className="mm-row-size">{m.size_display}</span>
                <span
                  className={`mm-row-status ${m.installed ? "ok" : "missing"}`}
                >
                  {m.installed ? "✓ installed" : "missing"}
                </span>
              </li>
            ))}
          </ul>
          <p className="mm-hint">
            Run the AUGUR Installer to add or remove models.
          </p>
        </div>
      </div>
    </div>
  );
}
