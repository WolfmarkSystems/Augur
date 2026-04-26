import { create } from "zustand";
import type {
  BatchFileRow,
  BatchProgress,
  CodeSwitchPoint,
  DialectInfo,
  FileKind,
  Language,
  ReviewStatus,
  SegmentFlag,
  TranslationSegment,
} from "../types";
import { ALL_LANGUAGES } from "../components/languages";

const findLang = (code: string): Language => {
  const found = ALL_LANGUAGES.find((l) => l.code === code);
  if (!found) {
    throw new Error(`Unknown language code: ${code}`);
  }
  return found;
};

export type SttModel =
  | "auto"
  | "whisper-tiny"
  | "whisper-base"
  | "whisper-large-v3"
  | "whisper-pashto"
  | "whisper-dari";

export type TranslationEngine =
  | "auto"
  | "nllb-600m"
  | "nllb-1b3"
  | "seamless-m4t";

export interface AppState {
  // File
  loadedFile: string | null;
  fileType: FileKind | null;
  fileName: string | null;
  fileSizeBytes: number | null;

  // Languages
  sourceLang: Language;
  targetLang: Language;

  // Models
  sttModel: SttModel;
  translationEngine: TranslationEngine;
  activeEngine: TranslationEngine | null;

  // Translation state
  isTranslating: boolean;
  segments: TranslationSegment[];
  dialect: DialectInfo | null;
  codeSwitches: CodeSwitchPoint[];
  hasCodeSwitching: boolean;
  overallProgress: number;
  errorMessage: string | null;

  // Case info
  caseNumber: string;
  examinerName: string;
  agency: string;

  // Sprint 13 P2 — batch mode
  batch: BatchProgress | null;

  // Sprint 13 P4 — startup health
  augurAvailable: boolean | null;
  augurBinaryPath: string | null;
  selfTestFails: string[];

  // Sprint 16 P2 — recent files
  recentFiles: Array<{
    path: string;
    openedAt: string;
    sourceLang: string;
    targetLang: string;
    fileType: string;
  }>;

  // Sprint 17 P1 — segment flags for the loaded file
  flaggedSegments: Record<number, SegmentFlag>;

  // View toggles
  showDialectCard: boolean;
  showCodeSwitchBands: boolean;
  forceTranscriptView: boolean;

  // Actions
  setSourceLang: (lang: Language) => void;
  setTargetLang: (lang: Language) => void;
  setSttModel: (model: SttModel) => void;
  setTranslationEngine: (engine: TranslationEngine) => void;
  setActiveEngine: (engine: TranslationEngine | null) => void;
  loadFile: (path: string, name: string, kind: FileKind, sizeBytes: number) => void;
  clearFile: () => void;
  resetTranslation: () => void;
  addSegment: (segment: TranslationSegment) => void;
  setDialect: (dialect: DialectInfo | null) => void;
  addCodeSwitch: (cs: CodeSwitchPoint) => void;
  setIsTranslating: (v: boolean) => void;
  setProgress: (p: number) => void;
  setError: (msg: string | null) => void;
  setCaseNumber: (n: string) => void;
  setExaminerName: (n: string) => void;
  setAgency: (n: string) => void;

  startBatch: (inputDir: string, outputPath: string, format: "html" | "json" | "csv" | "zip") => void;
  onBatchFileStart: (path: string, inputType: string, total: number) => void;
  onBatchFileDone: (row: Partial<BatchFileRow> & { path: string }) => void;
  onBatchComplete: (counts: {
    total: number;
    processed: number;
    foreign: number;
    translated: number;
    errors: number;
  }) => void;
  clearBatch: () => void;

  setAugurAvailable: (v: boolean, path: string | null) => void;
  setSelfTestFails: (fails: string[]) => void;
  setRecentFiles: (files: AppState["recentFiles"]) => void;

  flagSegment: (index: number, note: string) => void;
  unflagSegment: (index: number) => void;
  setReviewStatus: (index: number, status: ReviewStatus) => void;
  hydrateFlags: (flags: SegmentFlag[]) => void;

  toggleDialectCard: () => void;
  toggleCodeSwitchBands: () => void;
  setForceTranscriptView: (v: boolean) => void;
}

export const useAppStore = create<AppState>((set) => ({
  loadedFile: null,
  fileType: null,
  fileName: null,
  fileSizeBytes: null,

  sourceLang: findLang("ar"),
  targetLang: findLang("en"),

  sttModel: "auto",
  translationEngine: "auto",
  activeEngine: null,

  isTranslating: false,
  segments: [],
  dialect: null,
  codeSwitches: [],
  hasCodeSwitching: false,
  overallProgress: 0,
  errorMessage: null,

  caseNumber: "CASE-2026-0001",
  examinerName: "",
  agency: "",

  batch: null,
  augurAvailable: null,
  augurBinaryPath: null,
  selfTestFails: [],
  recentFiles: [],
  flaggedSegments: {},

  showDialectCard: true,
  showCodeSwitchBands: true,
  forceTranscriptView: false,

  setSourceLang: (lang) => set({ sourceLang: lang }),
  setTargetLang: (lang) => set({ targetLang: lang }),
  setSttModel: (model) => set({ sttModel: model }),
  setTranslationEngine: (engine) => set({ translationEngine: engine }),
  setActiveEngine: (engine) => set({ activeEngine: engine }),

  loadFile: (path, name, kind, sizeBytes) =>
    set({
      loadedFile: path,
      fileName: name,
      fileType: kind,
      fileSizeBytes: sizeBytes,
      segments: [],
      dialect: null,
      codeSwitches: [],
      hasCodeSwitching: false,
      overallProgress: 0,
      errorMessage: null,
      // Sprint 17 P1 — flags are per-file; clear on file change.
      // Restoration happens in App.tsx via getSegmentFlags() once
      // the load is acknowledged.
      flaggedSegments: {},
    }),
  clearFile: () =>
    set({
      loadedFile: null,
      fileName: null,
      fileType: null,
      fileSizeBytes: null,
    }),
  resetTranslation: () =>
    set({
      segments: [],
      dialect: null,
      codeSwitches: [],
      hasCodeSwitching: false,
      overallProgress: 0,
      errorMessage: null,
    }),
  addSegment: (segment) =>
    set((s) => ({ segments: [...s.segments, segment] })),
  setDialect: (dialect) => set({ dialect }),
  addCodeSwitch: (cs) =>
    set((s) => ({
      codeSwitches: [...s.codeSwitches, cs],
      hasCodeSwitching: true,
    })),
  setIsTranslating: (v) => set({ isTranslating: v }),
  setProgress: (p) => set({ overallProgress: p }),
  setError: (msg) => set({ errorMessage: msg }),
  setCaseNumber: (n) => set({ caseNumber: n }),
  setExaminerName: (n) => set({ examinerName: n }),
  setAgency: (n) => set({ agency: n }),

  startBatch: (inputDir, outputPath, format) =>
    set({
      batch: {
        inputDir,
        outputPath,
        format,
        total: 0,
        processed: 0,
        foreign: 0,
        translated: 0,
        errors: 0,
        files: [],
        isRunning: true,
      },
    }),
  onBatchFileStart: (path, inputType, total) =>
    set((s) => {
      if (!s.batch) return {};
      const name = path.split("/").pop() ?? path;
      const existing = s.batch.files.find((f) => f.path === path);
      const updated = existing
        ? s.batch.files.map((f) =>
            f.path === path ? { ...f, status: "active" as const } : f,
          )
        : [
            ...s.batch.files,
            {
              path,
              name,
              inputType,
              status: "active" as const,
            },
          ];
      return {
        batch: {
          ...s.batch,
          total: Math.max(s.batch.total, total),
          files: updated,
        },
      };
    }),
  onBatchFileDone: (row) =>
    set((s) => {
      if (!s.batch) return {};
      const path = row.path;
      const next = s.batch.files.map((f) =>
        f.path === path
          ? {
              ...f,
              ...row,
              status: row.error ? ("error" as const) : ("done" as const),
            }
          : f,
      );
      return {
        batch: {
          ...s.batch,
          files: next,
          processed: next.filter((f) => f.status === "done" || f.status === "error").length,
          foreign: next.filter((f) => f.isForeign).length,
          translated: next.filter((f) => f.translated).length,
          errors: next.filter((f) => f.status === "error").length,
        },
      };
    }),
  onBatchComplete: (counts) =>
    set((s) =>
      s.batch
        ? {
            batch: {
              ...s.batch,
              ...counts,
              isRunning: false,
            },
          }
        : {},
    ),
  clearBatch: () => set({ batch: null }),

  setAugurAvailable: (v, path) =>
    set({ augurAvailable: v, augurBinaryPath: path }),
  setSelfTestFails: (fails) => set({ selfTestFails: fails }),
  setRecentFiles: (files) => set({ recentFiles: files }),

  flagSegment: (index, note) =>
    set((s) => ({
      flaggedSegments: {
        ...s.flaggedSegments,
        [index]: {
          segmentIndex: index,
          flaggedAt: new Date().toISOString(),
          examinerNote: note,
          reviewStatus: "needs_review",
        },
      },
    })),
  unflagSegment: (index) =>
    set((s) => {
      const next = { ...s.flaggedSegments };
      delete next[index];
      return { flaggedSegments: next };
    }),
  setReviewStatus: (index, status) =>
    set((s) =>
      s.flaggedSegments[index]
        ? {
            flaggedSegments: {
              ...s.flaggedSegments,
              [index]: { ...s.flaggedSegments[index], reviewStatus: status },
            },
          }
        : {},
    ),
  hydrateFlags: (flags) =>
    set({
      flaggedSegments: flags.reduce<Record<number, SegmentFlag>>((acc, f) => {
        acc[f.segmentIndex] = f;
        return acc;
      }, {}),
    }),

  toggleDialectCard: () =>
    set((s) => ({ showDialectCard: !s.showDialectCard })),
  toggleCodeSwitchBands: () =>
    set((s) => ({ showCodeSwitchBands: !s.showCodeSwitchBands })),
  setForceTranscriptView: (v) => set({ forceTranscriptView: v }),
}));
