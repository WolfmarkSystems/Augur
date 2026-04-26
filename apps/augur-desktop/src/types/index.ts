export type FileKind = "document" | "audio" | "video" | "subtitle" | "image";
export type Quality = "hi" | "med" | "low";
export type Tier = "High quality" | "Forensic priority" | "Limited quality";

export interface Language {
  code: string;
  name: string;
  flag: string;
  quality: Quality;
  tier: Tier;
  sub?: string;
}

export interface TranslationSegment {
  index: number;
  startMs?: number;
  endMs?: number;
  originalText: string;
  translatedText: string;
  isComplete: boolean;
  isCodeSwitch?: boolean;
  switchFrom?: string;
  switchTo?: string;
  speakerId?: string;
}

export interface DialectInfo {
  dialect: string;
  confidence: number;
  source: "camel" | "lexical";
  indicators?: string[];
}

export interface CodeSwitchPoint {
  offset: number;
  from: string;
  to: string;
}

export interface ModelStatus {
  id: string;
  name: string;
  size_display: string;
  installed: boolean;
  tier: string;
}

export type BatchFileStatus = "waiting" | "active" | "done" | "error";

export interface BatchFileRow {
  path: string;
  name: string;
  inputType?: string;
  detectedLanguage?: string;
  isForeign?: boolean;
  translated?: boolean;
  error?: string | null;
  status: BatchFileStatus;
}

export type ReviewStatus = "needs_review" | "reviewed" | "disputed";

export interface SegmentFlag {
  segmentIndex: number;
  flaggedAt: string;
  examinerNote: string;
  reviewStatus: ReviewStatus;
}

export interface BatchProgress {
  inputDir: string | null;
  outputPath: string | null;
  format: "html" | "json" | "csv" | "zip";
  total: number;
  processed: number;
  foreign: number;
  translated: number;
  errors: number;
  files: BatchFileRow[];
  isRunning: boolean;
}
