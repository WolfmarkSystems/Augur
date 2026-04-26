import { useEffect, useMemo, useState } from "react";
import {
  createEvidencePackage,
  onPackageComplete,
  onPackageError,
  onPackageFileDone,
  onPackageFileStart,
  openDirectoryDialog,
  openEvidenceDialog,
  saveReportDialog,
} from "../ipc";
import { useAppStore } from "../store/appStore";

type WizardStep = 1 | 2 | 3 | 4 | 5;

interface FileRow {
  path: string;
  status: "waiting" | "active" | "done" | "error";
  detectedLanguage?: string;
  translated?: boolean;
  error?: string | null;
}

interface CompleteResult {
  outputPath: string;
  totalFiles: number;
  translatedFiles: number;
  errors: number;
  sizeBytes: number;
}

interface Props {
  open: boolean;
  onClose: () => void;
}

function shortPath(path: string): string {
  const parts = path.split("/");
  if (parts.length <= 3) return path;
  return `…/${parts.slice(-3).join("/")}`;
}

export default function PackageWizard({ open, onClose }: Props) {
  const targetLang = useAppStore((s) => s.targetLang);
  const caseNumber = useAppStore((s) => s.caseNumber);
  const examinerName = useAppStore((s) => s.examinerName);
  const agency = useAppStore((s) => s.agency);
  const setCaseNumber = useAppStore((s) => s.setCaseNumber);
  const setExaminer = useAppStore((s) => s.setExaminerName);
  const setAgency = useAppStore((s) => s.setAgency);
  const flaggedSegments = useAppStore((s) => s.flaggedSegments);
  const loadedFile = useAppStore((s) => s.loadedFile);

  const [step, setStep] = useState<WizardStep>(1);
  const [inputPath, setInputPath] = useState<string>("");
  const [inputKind, setInputKind] = useState<"file" | "folder" | null>(null);
  const [outputPath, setOutputPath] = useState<string>("");
  const [files, setFiles] = useState<FileRow[]>([]);
  const [progressTotal, setProgressTotal] = useState(0);
  const [progressDone, setProgressDone] = useState(0);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [result, setResult] = useState<CompleteResult | null>(null);

  // Reset on open.
  useEffect(() => {
    if (!open) return;
    setStep(1);
    setInputPath("");
    setInputKind(null);
    setOutputPath("");
    setFiles([]);
    setProgressTotal(0);
    setProgressDone(0);
    setErrorMsg(null);
    setResult(null);
  }, [open]);

  // Subscribe to package events.
  useEffect(() => {
    if (!open) return;
    const unlisten: Array<() => void> = [];
    onPackageFileStart(({ file, total }) => {
      setProgressTotal(total);
      setFiles((prev) => {
        const existing = prev.find((f) => f.path === file);
        if (existing) {
          return prev.map((f) =>
            f.path === file ? { ...f, status: "active" } : f,
          );
        }
        return [...prev, { path: file, status: "active" }];
      });
    }).then((u) => unlisten.push(u));
    onPackageFileDone((p) => {
      setFiles((prev) =>
        prev.map((f) =>
          f.path === p.file
            ? {
                ...f,
                status: p.error ? "error" : "done",
                detectedLanguage: p.detected_language,
                translated: p.translated,
                error: p.error,
              }
            : f,
        ),
      );
      setProgressDone(p.processed);
    }).then((u) => unlisten.push(u));
    onPackageComplete((p) => {
      setResult({
        outputPath: p.output_path,
        totalFiles: p.total_files,
        translatedFiles: p.translated_files,
        errors: p.errors,
        sizeBytes: p.size_bytes,
      });
      setStep(5);
    }).then((u) => unlisten.push(u));
    onPackageError((p) => setErrorMsg(p.message)).then((u) =>
      unlisten.push(u),
    );
    return () => unlisten.forEach((u) => u());
  }, [open]);

  const overallPct = useMemo(() => {
    if (progressTotal === 0) return 0;
    return Math.round((progressDone / progressTotal) * 100);
  }, [progressDone, progressTotal]);

  const handlePickFile = async () => {
    const p = await openEvidenceDialog().catch(() => null);
    if (p) {
      setInputPath(p);
      setInputKind("file");
    }
  };
  const handlePickFolder = async () => {
    const p = await openDirectoryDialog().catch(() => null);
    if (p) {
      setInputPath(p);
      setInputKind("folder");
    }
  };
  const handleChooseOutput = async () => {
    const p = await saveReportDialog({
      format: "zip",
      caseNumber: caseNumber || "package",
    }).catch(() => null);
    if (p) {
      setOutputPath(p);
    }
  };

  const handleStart = async () => {
    if (!inputPath || !outputPath) {
      setErrorMsg("Both an evidence path and an output path are required.");
      return;
    }
    setStep(4);
    try {
      const flagsForPackage = Object.values(flaggedSegments).map((f) => ({
        filePath: loadedFile ?? inputPath,
        segmentIndex: f.segmentIndex,
        examinerNote: f.examinerNote,
        reviewStatus: f.reviewStatus,
        flaggedAt: f.flaggedAt,
      }));
      await createEvidencePackage({
        inputPath,
        targetLang: targetLang.code,
        caseNumber,
        examinerName,
        agency,
        outputPath,
        flaggedSegments: flagsForPackage,
      });
    } catch (e) {
      setErrorMsg(`Could not start packaging: ${String(e)}`);
    }
  };

  if (!open) return null;

  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="overlay-backdrop" onClick={onClose} />
      <div className="overlay-panel overlay-panel-wide pkg-wizard">
        <header className="overlay-header">
          <span>Create Evidence Package — Step {step} of 5</span>
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
          {errorMsg && (
            <div className="pkg-error">⚠ {errorMsg}</div>
          )}

          {step === 1 && (
            <div className="pkg-step">
              <h3>Select evidence</h3>
              <div className="pkg-row">
                <button type="button" className="btn btn-primary" onClick={handlePickFile}>
                  Select File
                </button>
                <button type="button" className="btn" onClick={handlePickFolder}>
                  Select Folder
                </button>
              </div>
              {inputPath && (
                <div className="pkg-hint">
                  Selected: <code>{inputPath}</code>{" "}
                  ({inputKind === "folder" ? "directory" : "single file"})
                </div>
              )}
              <div className="pkg-actions">
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => setStep(2)}
                  disabled={!inputPath}
                >
                  Next →
                </button>
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="pkg-step">
              <h3>Case information</h3>
              <label className="pkg-field">
                <span>Case Number</span>
                <input
                  type="text"
                  value={caseNumber}
                  onChange={(e) => setCaseNumber(e.target.value)}
                  placeholder="2026-042"
                />
              </label>
              <label className="pkg-field">
                <span>Examiner</span>
                <input
                  type="text"
                  value={examinerName}
                  onChange={(e) => setExaminer(e.target.value)}
                  placeholder="D. Examiner"
                />
              </label>
              <label className="pkg-field">
                <span>Agency</span>
                <input
                  type="text"
                  value={agency}
                  onChange={(e) => setAgency(e.target.value)}
                  placeholder="Wolfmark Systems"
                />
              </label>
              <p className="pkg-hint">
                These fields land in <code>MANIFEST.json</code> and
                <code> CHAIN_OF_CUSTODY.txt</code>. Leaving any blank is
                permitted; the field is omitted from the manifest.
              </p>
              <div className="pkg-actions">
                <button type="button" className="btn" onClick={() => setStep(1)}>
                  Back
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => setStep(3)}
                >
                  Next →
                </button>
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="pkg-step">
              <h3>Output</h3>
              <div className="pkg-row">
                <button type="button" className="btn btn-primary" onClick={handleChooseOutput}>
                  Choose Output Path…
                </button>
              </div>
              {outputPath && (
                <div className="pkg-hint">
                  Saving to: <code>{outputPath}</code>
                </div>
              )}
              <p className="pkg-hint">
                Format: <strong>ZIP</strong> (forensic chain-of-custody
                package).
              </p>
              <div className="pkg-actions">
                <button type="button" className="btn" onClick={() => setStep(2)}>
                  Back
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={handleStart}
                  disabled={!outputPath}
                >
                  Start Packaging →
                </button>
              </div>
            </div>
          )}

          {step === 4 && (
            <div className="pkg-step">
              <h3>Packaging…</h3>
              <div className="pkg-progress-files">
                {files.map((f, idx) => (
                  <div key={f.path} className={`pkg-file is-${f.status}`}>
                    <span className="pkg-file-idx">[{idx + 1}/{Math.max(progressTotal, files.length)}]</span>
                    <span className="pkg-file-name" title={f.path}>
                      {shortPath(f.path)}
                    </span>
                    <span className="pkg-file-status">
                      {f.status === "active" && <span className="spinner" />}
                      {f.status === "done" && (
                        <span className="check">{f.translated ? "✓ translated" : "✓"}</span>
                      )}
                      {f.status === "error" && (
                        <span className="batch-row-err">! {f.error?.slice(0, 40)}</span>
                      )}
                    </span>
                  </div>
                ))}
              </div>
              <div className="overall-progress">
                <div
                  className="overall-progress-fill"
                  style={{ width: `${overallPct}%` }}
                />
              </div>
              <div className="pkg-hint">
                Overall: {progressDone}/{progressTotal} · {overallPct}% complete
              </div>
            </div>
          )}

          {step === 5 && result && (
            <div className="pkg-step">
              <div className="complete-icon" aria-hidden="true">✓</div>
              <h3>Package created</h3>
              <p className="pkg-hint">
                Saved <strong>{result.totalFiles}</strong> files
                ({result.translatedFiles} translated, {result.errors}{" "}
                error{result.errors === 1 ? "" : "s"}, {(result.sizeBytes / 1e6).toFixed(1)} MB).
              </p>
              <div className="pkg-output-path">
                <code>{result.outputPath}</code>
              </div>
              <div className="mt-advisory">
                <strong>Forensic notice:</strong> Machine translation —
                verify with a certified human translator for legal
                proceedings. The package contains MANIFEST.json,
                CHAIN_OF_CUSTODY.txt, and per-file translations.
              </div>
              <div className="pkg-actions">
                <button type="button" className="btn btn-primary" onClick={onClose}>
                  Close
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
