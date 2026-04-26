import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../store/appStore";
import {
  exportReport,
  openDirectoryDialog,
  openEvidenceDialog,
  loadFileMetadata,
  saveReportDialog,
  startBatchTranslation,
} from "../ipc";
import type { FileKind } from "../types";

function shortName(p: string): string {
  const segs = p.split("/");
  return segs[segs.length - 1] ?? p;
}

interface Props {
  onOpenModelManager: () => void;
  onOpenAdvisory: () => void;
  onSetCaseNumber: () => void;
  onOpenPackageWizard: () => void;
}

export default function MenuBar({
  onOpenModelManager,
  onOpenAdvisory,
  onSetCaseNumber,
  onOpenPackageWizard,
}: Props) {
  const [open, setOpen] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(null);
    };
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(null);
      }
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onClick);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onClick);
    };
  }, [open]);

  const loadFile = useAppStore((s) => s.loadFile);
  const setForceTranscript = useAppStore((s) => s.setForceTranscriptView);
  const toggleDialect = useAppStore((s) => s.toggleDialectCard);
  const toggleBands = useAppStore((s) => s.toggleCodeSwitchBands);
  const segments = useAppStore((s) => s.segments);
  const sourceLang = useAppStore((s) => s.sourceLang);
  const targetLang = useAppStore((s) => s.targetLang);
  const dialect = useAppStore((s) => s.dialect);
  const caseNumber = useAppStore((s) => s.caseNumber);
  const setError = useAppStore((s) => s.setError);
  const startBatch = useAppStore((s) => s.startBatch);
  const targetLang = useAppStore((s) => s.targetLang);
  const recentFiles = useAppStore((s) => s.recentFiles);
  const loadFile = useAppStore((s) => s.loadFile);

  const handleOpen = async () => {
    setOpen(null);
    try {
      const path = await openEvidenceDialog();
      if (!path) return;
      const meta = await loadFileMetadata(path);
      loadFile(meta.path, meta.name, meta.kind as FileKind, meta.size_bytes);
    } catch (err) {
      setError(`Could not open file: ${String(err)}`);
    }
  };

  const handleOpenFolder = async () => {
    setOpen(null);
    try {
      const dir = await openDirectoryDialog();
      if (!dir) return;
      const stamp = new Date()
        .toISOString()
        .replace(/[-:.TZ]/g, "")
        .slice(0, 15);
      const outPath = `${dir}/AUGUR_batch_${stamp}.json`;
      startBatch(dir, outPath, "json");
      await startBatchTranslation({
        inputDir: dir,
        targetLang: targetLang.code,
        outputPath: outPath,
        format: "json",
      });
    } catch (err) {
      setError(`Could not start batch: ${String(err)}`);
    }
  };

  const handleExport = async (format: "html" | "json" | "zip") => {
    setOpen(null);
    try {
      const out = await saveReportDialog({ format, caseNumber });
      if (!out) return;
      const segPayload = segments.map((s) => ({
        index: s.index,
        startMs: s.startMs ?? null,
        endMs: s.endMs ?? null,
        originalText: s.originalText,
        translatedText: s.translatedText,
        speakerId: s.speakerId ?? null,
      }));
      await exportReport({
        format,
        outputPath: out,
        caseNumber,
        sourceLang: sourceLang.code,
        targetLang: targetLang.code,
        dialect: dialect ? dialect.dialect : null,
        segments: segPayload,
      });
    } catch (err) {
      setError(`Export failed: ${String(err)}`);
    }
  };

  const Item = ({
    label,
    onClick,
    shortcut,
  }: {
    label: string;
    onClick: () => void;
    shortcut?: string;
  }) => (
    <button type="button" className="menu-item" onClick={onClick}>
      <span>{label}</span>
      {shortcut && <span className="menu-shortcut">{shortcut}</span>}
    </button>
  );

  return (
    <div className="menubar" ref={ref}>
      {(["File", "View", "Models", "Help"] as const).map((m) => (
        <div
          key={m}
          className={`menu ${open === m ? "is-open" : ""}`}
        >
          <button
            type="button"
            className="menu-button"
            onClick={() => setOpen(open === m ? null : m)}
          >
            {m}
          </button>
          {open === m && (
            <div className="menu-dropdown">
              {m === "File" && (
                <>
                  <Item
                    label="Open Evidence…"
                    onClick={handleOpen}
                    shortcut="⌘O"
                  />
                  <Item
                    label="Open Folder… (batch)"
                    onClick={handleOpenFolder}
                    shortcut="⌘⇧O"
                  />
                  {recentFiles.length > 0 && (
                    <>
                      <div className="menu-divider" />
                      <div className="menu-section-label">Recent Files</div>
                      {recentFiles
                        .slice()
                        .reverse()
                        .slice(0, 10)
                        .map((rf) => (
                          <Item
                            key={rf.path}
                            label={`${shortName(rf.path)}  (${rf.sourceLang} → ${rf.targetLang})`}
                            onClick={async () => {
                              setOpen(null);
                              try {
                                const meta = await loadFileMetadata(rf.path);
                                loadFile(
                                  meta.path,
                                  meta.name,
                                  meta.kind as FileKind,
                                  meta.size_bytes,
                                );
                              } catch (err) {
                                setError(`Could not reopen ${rf.path}: ${String(err)}`);
                              }
                            }}
                          />
                        ))}
                    </>
                  )}
                  <div className="menu-divider" />
                  <Item
                    label="Export Report → HTML"
                    onClick={() => handleExport("html")}
                  />
                  <Item
                    label="Export Report → JSON"
                    onClick={() => handleExport("json")}
                  />
                  <Item
                    label="Export ZIP package"
                    onClick={() => handleExport("zip")}
                  />
                  <Item
                    label="Create Evidence Package…"
                    onClick={() => {
                      setOpen(null);
                      onOpenPackageWizard();
                    }}
                    shortcut="⌘E"
                  />
                  <div className="menu-divider" />
                  <Item
                    label="Set Case Number…"
                    onClick={() => {
                      setOpen(null);
                      onSetCaseNumber();
                    }}
                  />
                </>
              )}
              {m === "View" && (
                <>
                  <Item
                    label="Document View"
                    onClick={() => {
                      setForceTranscript(false);
                      setOpen(null);
                    }}
                  />
                  <Item
                    label="Transcript View"
                    onClick={() => {
                      setForceTranscript(true);
                      setOpen(null);
                    }}
                  />
                  <div className="menu-divider" />
                  <Item
                    label="Toggle Dialect Card"
                    onClick={() => {
                      toggleDialect();
                      setOpen(null);
                    }}
                  />
                  <Item
                    label="Toggle Code-Switch Bands"
                    onClick={() => {
                      toggleBands();
                      setOpen(null);
                    }}
                  />
                </>
              )}
              {m === "Models" && (
                <>
                  <Item
                    label="Open Model Manager"
                    onClick={() => {
                      setOpen(null);
                      onOpenModelManager();
                    }}
                  />
                </>
              )}
              {m === "Help" && (
                <>
                  <Item
                    label="About AUGUR"
                    onClick={() => {
                      setOpen(null);
                      window.alert("AUGUR v1.0.0 — Wolfmark Systems");
                    }}
                  />
                  <Item
                    label="MT Advisory Notice"
                    onClick={() => {
                      setOpen(null);
                      onOpenAdvisory();
                    }}
                  />
                </>
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
