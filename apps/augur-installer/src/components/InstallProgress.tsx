import { useEffect, useState } from "react";
import {
  getProfileComponents,
  onComponentDone,
  onComponentError,
  onComponentStart,
  onDownloadProgress,
  onInstallComplete,
  startInstallation,
} from "../ipc";
import type {
  ComponentState,
  InstallComponent,
  InstallResult,
  Profile,
} from "../types";

function iconFor(componentType: string): string {
  switch (componentType) {
    case "Runtime":
      return "🐍";
    case "BundledBin":
      return "⚙";
    case "SttModel":
      return "🎙";
    case "TransModel":
      return "🌐";
    case "Classifier":
      return "🔍";
    case "Diarization":
      return "👥";
    default:
      return "📦";
  }
}

function formatEta(s: number): string {
  if (s <= 0) return "";
  if (s < 60) return `${s}s remaining`;
  return `${Math.round(s / 60)}m remaining`;
}

function ComponentRow({ component }: { component: ComponentState }) {
  return (
    <div className={`install-item is-${component.status}`}>
      <div className="ii-icon" aria-hidden="true">
        {iconFor(component.componentType)}
      </div>
      <div className="ii-info">
        <div className="ii-name">{component.name}</div>
        <div className="ii-sub">
          {component.status === "active" && component.speedMbps > 0
            ? `${component.speedMbps.toFixed(1)} MB/s · ${formatEta(component.etaSeconds)}`
            : component.description}
        </div>
        {component.status === "active" && (
          <div className="item-progress" aria-hidden="true">
            <div
              className="item-progress-fill"
              style={{ width: `${Math.max(0, Math.min(100, component.percent))}%` }}
            />
          </div>
        )}
        {component.status === "error" && (
          <div className="ii-error">Failed — see logs.</div>
        )}
      </div>
      <div className="ii-size">{component.sizeDisplay}</div>
      <div className="ii-status" aria-hidden="true">
        {component.status === "waiting" && <div className="waiting-dot" />}
        {component.status === "active" && <div className="spinner" />}
        {component.status === "done" && <div className="check">✓</div>}
        {component.status === "error" && <div className="error-dot">!</div>}
      </div>
    </div>
  );
}

export default function InstallProgress({
  profile,
  onStart,
  onComplete,
}: {
  profile: Profile;
  onStart: () => void;
  onComplete: (result: InstallResult) => void;
}) {
  const [components, setComponents] = useState<ComponentState[]>([]);
  const [overallPct, setOverallPct] = useState(0);
  const [currentLabel, setCurrentLabel] = useState("Preparing…");

  useEffect(() => {
    let cancelled = false;
    const unlisten: Array<() => void> = [];

    async function run() {
      try {
        const comps = await getProfileComponents(profile);
        if (cancelled) return;
        const initial: ComponentState[] = comps.map((c: InstallComponent) => ({
          ...c,
          status: "waiting",
          percent: 0,
          speedMbps: 0,
          etaSeconds: 0,
        }));
        setComponents(initial);

        unlisten.push(
          await onComponentStart(({ id }) => {
            setComponents((prev) =>
              prev.map((c) =>
                c.id === id ? { ...c, status: "active" } : c,
              ),
            );
            const found = initial.find((c) => c.id === id);
            setCurrentLabel(`Installing: ${found?.name ?? id}`);
          }),
        );
        unlisten.push(
          await onDownloadProgress(({ component_id, percent, speed_mbps, eta_seconds }) => {
            setComponents((prev) =>
              prev.map((c) =>
                c.id === component_id
                  ? {
                      ...c,
                      percent,
                      speedMbps: speed_mbps,
                      etaSeconds: eta_seconds,
                    }
                  : c,
              ),
            );
          }),
        );
        unlisten.push(
          await onComponentDone(({ id, index }) => {
            setComponents((prev) =>
              prev.map((c) =>
                c.id === id ? { ...c, status: "done", percent: 100 } : c,
              ),
            );
            const total = initial.length;
            setOverallPct(Math.round(((index + 1) / total) * 100));
          }),
        );
        unlisten.push(
          await onComponentError(({ id, error }) => {
            setComponents((prev) =>
              prev.map((c) =>
                c.id === id
                  ? { ...c, status: "error", description: error }
                  : c,
              ),
            );
            setCurrentLabel(`Error installing ${id}`);
          }),
        );
        unlisten.push(
          await onInstallComplete(() => {
            setOverallPct(100);
            setCurrentLabel("Installation complete");
            const totalBytes = initial.reduce(
              (acc, c) => acc + c.sizeBytes,
              0,
            );
            setTimeout(
              () =>
                onComplete({
                  profile,
                  componentCount: initial.length,
                  totalBytes,
                }),
              600,
            );
          }),
        );

        onStart();
        await startInstallation(profile);
      } catch (err) {
        setCurrentLabel(`Setup failed: ${String(err)}`);
      }
    }

    run();
    return () => {
      cancelled = true;
      unlisten.forEach((u) => u());
    };
  }, [profile, onStart, onComplete]);

  return (
    <div className="install-screen">
      <div className="install-header">
        <h2 className="screen-title">Installing AUGUR</h2>
        <p className="screen-sub">{currentLabel}</p>
      </div>
      <div className="install-list">
        {components.map((c) => (
          <ComponentRow key={c.id} component={c} />
        ))}
      </div>
      <div className="overall-progress" aria-label="Overall progress">
        <div
          className="overall-progress-fill"
          style={{ width: `${overallPct}%` }}
        />
      </div>
      <div className="overall-progress-label">{overallPct}% complete</div>
    </div>
  );
}
