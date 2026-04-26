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
