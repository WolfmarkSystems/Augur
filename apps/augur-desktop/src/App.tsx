import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "./store/appStore";
import TitleBar from "./components/TitleBar";
import MenuBar from "./components/MenuBar";
import Toolbar from "./components/Toolbar";
import StatusBar from "./components/StatusBar";
import WorkspaceDoc from "./components/WorkspaceDoc";
import WorkspaceAudio from "./components/WorkspaceAudio";
import ModelManager from "./components/ModelManager";
import {
  mtAdvisoryText,
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

  const [showModelManager, setShowModelManager] = useState(false);
  const [showAdvisory, setShowAdvisory] = useState(false);
  const [advisoryText, setAdvisoryText] = useState<string>(MT_ADVISORY_FALLBACK);

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
      setIsTranslating(false);
      setError(p.error);
    }).then((u) => unlisten.push(u));
    mtAdvisoryText()
      .then((t) => setAdvisoryText(t))
      .catch(() => setAdvisoryText(MT_ADVISORY_FALLBACK));
    return () => unlisten.forEach((u) => u());
  }, [
    addSegment,
    setDialect,
    addCodeSwitch,
    setIsTranslating,
    setActiveEngine,
    setProgress,
    setError,
  ]);

  // Whenever a new file is loaded, kick off translation.
  useEffect(() => {
    if (!loadedFile) return;
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
        onSetCaseNumber={() => {
          const next = window.prompt(
            "Case number for exports and chain of custody:",
            caseNumber,
          );
          if (next && next.trim()) setCaseNumber(next.trim());
        }}
      />
      <Toolbar />
      <main className="app-body">
        {useAudioWorkspace ? <WorkspaceAudio /> : <WorkspaceDoc />}
      </main>
      <StatusBar />

      <ModelManager
        open={showModelManager}
        onClose={() => setShowModelManager(false)}
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
