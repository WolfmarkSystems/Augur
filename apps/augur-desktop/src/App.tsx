import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "./store/appStore";
import TitleBar from "./components/TitleBar";
import MenuBar from "./components/MenuBar";
import Toolbar from "./components/Toolbar";
import StatusBar from "./components/StatusBar";
import WorkspaceDoc from "./components/WorkspaceDoc";
import WorkspaceAudio from "./components/WorkspaceAudio";
import WorkspaceBatch from "./components/WorkspaceBatch";
import ModelManager from "./components/ModelManager";
import PackageWizard from "./components/PackageWizard";
import ReviewPanel from "./components/ReviewPanel";
import ErrorBanner, { type ErrorBannerType } from "./components/ErrorBanner";
import { invoke } from "@tauri-apps/api/core";
import {
  addRecentFile,
  augurBinaryPath,
  checkAugurAvailable,
  getCaseState,
  getSegmentFlags,
  saveSegmentFlags,
  setCaseInfo,
  mtAdvisoryText,
  onBatchComplete,
  onBatchError,
  onBatchFileDone,
  onBatchFileStart,
  onCodeSwitchDetected,
  onDialectDetected,
  onSegmentReady,
  onTranslationComplete,
  onTranslationError,
  startTranslation,
} from "./ipc";

const MT_ADVISORY_FALLBACK =
  "Machine translation — verify with a certified human translator for legal proceedings.";

export default function App() {
  const loadedFile = useAppStore((s) => s.loadedFile);
  const fileType = useAppStore((s) => s.fileType);
  const sourceLang = useAppStore((s) => s.sourceLang);
  const targetLang = useAppStore((s) => s.targetLang);
  const sttModel = useAppStore((s) => s.sttModel);
  const engine = useAppStore((s) => s.translationEngine);
  const setActiveEngine = useAppStore((s) => s.setActiveEngine);
  const setIsTranslating = useAppStore((s) => s.setIsTranslating);
  const addSegment = useAppStore((s) => s.addSegment);
  const setDialect = useAppStore((s) => s.setDialect);
  const addCodeSwitch = useAppStore((s) => s.addCodeSwitch);
  const resetTranslation = useAppStore((s) => s.resetTranslation);
  const setError = useAppStore((s) => s.setError);
  const setProgress = useAppStore((s) => s.setProgress);
  const forceTranscript = useAppStore((s) => s.forceTranscriptView);
  const caseNumber = useAppStore((s) => s.caseNumber);
  const setCaseNumber = useAppStore((s) => s.setCaseNumber);
  const batch = useAppStore((s) => s.batch);
  const onBatchFileStartStore = useAppStore((s) => s.onBatchFileStart);
  const onBatchFileDoneStore = useAppStore((s) => s.onBatchFileDone);
  const onBatchCompleteStore = useAppStore((s) => s.onBatchComplete);
  const setAugurAvailable = useAppStore((s) => s.setAugurAvailable);
  const augurAvailable = useAppStore((s) => s.augurAvailable);

  const [showModelManager, setShowModelManager] = useState(false);
  const [showPackageWizard, setShowPackageWizard] = useState(false);
  const [showAdvisory, setShowAdvisory] = useState(false);
  const [advisoryText, setAdvisoryText] = useState<string>(MT_ADVISORY_FALLBACK);
  const [bannerType, setBannerType] = useState<ErrorBannerType | null>(null);
  const [bannerMessage, setBannerMessage] = useState<string | undefined>();
  const setSelfTestFailsStore = useAppStore((s) => s.setSelfTestFails);
  const examinerName = useAppStore((s) => s.examinerName);
  const agency = useAppStore((s) => s.agency);
  const setExaminerStore = useAppStore((s) => s.setExaminerName);
  const setAgencyStore = useAppStore((s) => s.setAgency);
  const setRecentFiles = useAppStore((s) => s.setRecentFiles);
  const flaggedSegments = useAppStore((s) => s.flaggedSegments);
  const hydrateFlags = useAppStore((s) => s.hydrateFlags);

  // Subscribe to pipeline events once on mount.
  useEffect(() => {
    const unlisten: Array<() => void> = [];
    onSegmentReady((p) => {
      addSegment({
        index: p.index,
        startMs: p.start_ms ?? undefined,
        endMs: p.end_ms ?? undefined,
        originalText: p.original_text,
        translatedText: p.translated_text,
        isComplete: p.is_complete,
      });
    }).then((u) => unlisten.push(u));
    onDialectDetected((p) =>
      setDialect({
        dialect: p.dialect,
        confidence: p.confidence,
        source: p.source === "camel" ? "camel" : "lexical",
      }),
    ).then((u) => unlisten.push(u));
    onCodeSwitchDetected((p) =>
      addCodeSwitch({ offset: p.offset, from: p.from, to: p.to }),
    ).then((u) => unlisten.push(u));
    onTranslationComplete((p) => {
      setIsTranslating(false);
      setProgress(100);
      setActiveEngine(null);
      // total_segments is informational; the segments array is
      // already populated via segment-ready events.
      void p.total_segments;
    }).then((u) => unlisten.push(u));
    onTranslationError((p) => {
      const msg = p.message ?? p.error ?? "Translation failed";
      setIsTranslating(false);
      setError(msg);
      setBannerType("translation-failed");
      setBannerMessage(msg);
    }).then((u) => unlisten.push(u));
    onBatchFileStart((p) =>
      onBatchFileStartStore(p.file, p.input_type, p.total),
    ).then((u) => unlisten.push(u));
    onBatchFileDone((p) =>
      onBatchFileDoneStore({
        path: p.file,
        inputType: p.input_type,
        detectedLanguage: p.detected_language,
        isForeign: p.is_foreign,
        translated: p.translated,
        error: p.error,
        status: p.error ? "error" : "done",
      }),
    ).then((u) => unlisten.push(u));
    onBatchComplete((p) =>
      onBatchCompleteStore({
        total: p.total_files,
        processed: p.processed,
        foreign: p.foreign_files,
        translated: p.translated,
        errors: p.errors,
      }),
    ).then((u) => unlisten.push(u));
    onBatchError((p) => setError(`Batch failed: ${p.message}`)).then((u) =>
      unlisten.push(u),
    );
    // Sprint 13 P4 — startup probe for the augur CLI + non-
    // blocking self-test. Surfaces the four error states named
    // in the sprint spec.
    Promise.all([checkAugurAvailable(), augurBinaryPath()])
      .then(([avail, path]) => {
        setAugurAvailable(avail, path);
        if (!avail) {
          setBannerType("cli-not-found");
          setBannerMessage(undefined);
          return;
        }
        // CLI present — kick off the self-test. Failures stay
        // in the status bar; we only escalate to the banner on
        // a hard "models missing" pattern.
        invoke<string[]>("run_startup_self_test")
          .then((fails) => {
            setSelfTestFailsStore(fails);
            const modelsMissing = fails.some((f) =>
              f.toLowerCase().includes("not cached"),
            );
            if (modelsMissing && fails.length > 1) {
              setBannerType("models-missing");
              setBannerMessage(fails.slice(0, 3).join("\n"));
            }
          })
          .catch(() => {
            // self-test failure is non-fatal; clear the list.
            setSelfTestFailsStore([]);
          });
      })
      .catch(() => setAugurAvailable(false, null));
    mtAdvisoryText()
      .then((t) => setAdvisoryText(t))
      .catch(() => setAdvisoryText(MT_ADVISORY_FALLBACK));
    // Sprint 16 P2 — restore persistent case state.
    getCaseState()
      .then((s) => {
        if (s.case_number) setCaseNumber(s.case_number);
        if (s.examiner_name) setExaminerStore(s.examiner_name);
        if (s.agency) setAgencyStore(s.agency);
        setRecentFiles(
          s.recent_files.map((r) => ({
            path: r.path,
            openedAt: r.opened_at,
            sourceLang: r.source_lang,
            targetLang: r.target_lang,
            fileType: r.file_type,
          })),
        );
      })
      .catch(() => {
        // No on-disk case state yet; keep defaults.
      });
    return () => unlisten.forEach((u) => u());
  }, [
    addSegment,
    setDialect,
    addCodeSwitch,
    setIsTranslating,
    setActiveEngine,
    setProgress,
    setError,
    onBatchFileStartStore,
    onBatchFileDoneStore,
    onBatchCompleteStore,
    setAugurAvailable,
    setSelfTestFailsStore,
    setCaseNumber,
    setExaminerStore,
    setAgencyStore,
    setRecentFiles,
  ]);

  // Sprint 16 P2 — persist case info changes to disk.
  useEffect(() => {
    setCaseInfo({
      caseNumber,
      examinerName,
      agency,
    }).catch(() => {
      // Disk persistence failure is non-fatal — surface only
      // through logs, not a banner.
    });
  }, [caseNumber, examinerName, agency]);

  // Sprint 17 P1 — restore flags whenever the loaded file
  // changes; persist whenever the flag map changes.
  useEffect(() => {
    if (!loadedFile) return;
    getSegmentFlags({ filePath: loadedFile })
      .then((raw) => {
        const flags = (raw as Array<Record<string, unknown>>).map((r) => ({
          segmentIndex: Number(r.segmentIndex ?? r.segment_index ?? 0),
          flaggedAt: String(r.flaggedAt ?? r.flagged_at ?? ""),
          examinerNote: String(r.examinerNote ?? r.examiner_note ?? ""),
          reviewStatus: (r.reviewStatus ?? r.review_status ?? "needs_review") as
            | "needs_review"
            | "reviewed"
            | "disputed",
        }));
        hydrateFlags(flags);
      })
      .catch(() => hydrateFlags([]));
  }, [loadedFile, hydrateFlags]);

  useEffect(() => {
    if (!loadedFile) return;
    const flags = Object.values(flaggedSegments).map((f) => ({
      segmentIndex: f.segmentIndex,
      flaggedAt: f.flaggedAt,
      examinerNote: f.examinerNote,
      reviewStatus: f.reviewStatus,
    }));
    saveSegmentFlags({ filePath: loadedFile, flags }).catch(() => {});
  }, [loadedFile, flaggedSegments]);

  // Whenever a new file is loaded, kick off translation +
  // record it in the persistent recent-files list.
  useEffect(() => {
    if (!loadedFile) return;
    addRecentFile({
      path: loadedFile,
      sourceLang: sourceLang.code,
      targetLang: targetLang.code,
      fileType: fileType ?? "document",
    })
      .then(() => getCaseState())
      .then((s) =>
        setRecentFiles(
          s.recent_files.map((r) => ({
            path: r.path,
            openedAt: r.opened_at,
            sourceLang: r.source_lang,
            targetLang: r.target_lang,
            fileType: r.file_type,
          })),
        ),
      )
      .catch(() => {});
    resetTranslation();
    setIsTranslating(true);
    setActiveEngine(engine === "auto" ? "nllb-600m" : engine);
    startTranslation({
      filePath: loadedFile,
      sourceLang: sourceLang.code,
      targetLang: targetLang.code,
      sttModel,
      engine,
    }).catch((err) => {
      setIsTranslating(false);
      setError(`Pipeline failed: ${String(err)}`);
    });
  }, [
    loadedFile,
    sourceLang.code,
    targetLang.code,
    sttModel,
    engine,
    resetTranslation,
    setIsTranslating,
    setActiveEngine,
    setError,
  ]);

  const useAudioWorkspace = useMemo(
    () =>
      forceTranscript ||
      fileType === "audio" ||
      fileType === "video" ||
      fileType === "subtitle",
    [forceTranscript, fileType],
  );

  return (
    <div className="app">
      <TitleBar />
      <MenuBar
        onOpenModelManager={() => setShowModelManager(true)}
        onOpenAdvisory={() => setShowAdvisory(true)}
        onOpenPackageWizard={() => setShowPackageWizard(true)}
        onSetCaseNumber={() => {
          const next = window.prompt(
            "Case number for exports and chain of custody:",
            caseNumber,
          );
          if (next && next.trim()) setCaseNumber(next.trim());
        }}
      />
      <Toolbar />
      <ErrorBanner
        type={bannerType}
        message={bannerMessage}
        onDismiss={() => {
          setBannerType(null);
          setBannerMessage(undefined);
        }}
        actionLabel={
          bannerType === "models-missing" || bannerType === "translation-failed"
            ? "Open Model Manager"
            : undefined
        }
        onAction={
          bannerType === "models-missing" || bannerType === "translation-failed"
            ? () => {
                setShowModelManager(true);
                setBannerType(null);
              }
            : undefined
        }
      />
      <main className="app-body">
        {batch ? (
          <WorkspaceBatch />
        ) : useAudioWorkspace ? (
          <WorkspaceAudio />
        ) : (
          <WorkspaceDoc />
        )}
        <ReviewPanel />
      </main>
      {augurAvailable === false && bannerType !== "cli-not-found" && (
        <div className="cli-banner" role="alert">
          ⚠ AUGUR CLI not found on this system. Run the AUGUR Installer or
          install via <code>cargo install augur</code>.
        </div>
      )}
      <StatusBar />

      <ModelManager
        open={showModelManager}
        onClose={() => setShowModelManager(false)}
      />

      <PackageWizard
        open={showPackageWizard}
        onClose={() => setShowPackageWizard(false)}
      />

      {showAdvisory && (
        <div className="overlay" role="dialog" aria-modal="true">
          <div
            className="overlay-backdrop"
            onClick={() => setShowAdvisory(false)}
          />
          <div className="overlay-panel">
            <header className="overlay-header">
              <span>Machine-Translation Advisory</span>
              <button
                type="button"
                className="overlay-close"
                onClick={() => setShowAdvisory(false)}
                aria-label="Close"
              >
                ×
              </button>
            </header>
            <div className="overlay-body">
              <p className="advisory-paragraph">{advisoryText}</p>
              <p className="advisory-paragraph">
                AUGUR is an offline forensic translation tool. Every output
                you see in the right-hand panel is produced by a machine
                translation pipeline. <strong>It is not a substitute for
                review by a certified human translator</strong>, and must not
                be presented as such in legal proceedings.
              </p>
              <p className="advisory-paragraph">
                The MT advisory cannot be dismissed. It appears in the
                status bar, in every exported HTML / JSON report, and in
                every ZIP package manifest. This is a hard system rule.
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
