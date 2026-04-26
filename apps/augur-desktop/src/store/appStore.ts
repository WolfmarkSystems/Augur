import { create } from "zustand";
import type {
  CodeSwitchPoint,
  DialectInfo,
  FileKind,
  Language,
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

  toggleDialectCard: () =>
    set((s) => ({ showDialectCard: !s.showDialectCard })),
  toggleCodeSwitchBands: () =>
    set((s) => ({ showCodeSwitchBands: !s.showCodeSwitchBands })),
  setForceTranscriptView: (v) => set({ forceTranscriptView: v }),
}));
