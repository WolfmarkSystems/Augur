import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ModelStatus } from "../types";

export async function openEvidenceDialog(): Promise<string | null> {
  return invoke<string | null>("open_evidence_dialog");
}

export async function detectFileType(path: string): Promise<string> {
  return invoke<string>("detect_file_type", { path });
}

export async function loadFileMetadata(path: string): Promise<{
  path: string;
  name: string;
  kind: string;
  size_bytes: number;
}> {
  return invoke("load_file_metadata", { path });
}

export async function listModels(): Promise<ModelStatus[]> {
  return invoke<ModelStatus[]>("list_models");
}

export async function detectedProfile(): Promise<string> {
  return invoke<string>("detected_profile");
}

export async function startTranslation(args: {
  filePath: string;
  sourceLang: string;
  targetLang: string;
  sttModel: string;
  engine: string;
}): Promise<void> {
  return invoke<void>("start_translation", {
    filePath: args.filePath,
    sourceLang: args.sourceLang,
    targetLang: args.targetLang,
    sttModel: args.sttModel,
    engine: args.engine,
  });
}

export async function saveReportDialog(args: {
  format: "html" | "json" | "zip";
  caseNumber: string;
}): Promise<string | null> {
  return invoke<string | null>("save_report_dialog", args);
}

export async function exportReport(args: {
  format: "html" | "json" | "zip";
  outputPath: string;
  caseNumber: string;
  sourceLang: string;
  targetLang: string;
  dialect: string | null;
  segments: unknown[];
}): Promise<string> {
  return invoke<string>("export_report", args);
}

export async function mtAdvisoryText(): Promise<string> {
  return invoke<string>("mt_advisory_text");
}

// ── Pipeline event listeners ────────────────────────────────────

export function onSegmentReady(
  cb: (p: {
    index: number;
    start_ms: number | null;
    end_ms: number | null;
    original_text: string;
    translated_text: string;
    is_complete: boolean;
  }) => void,
): Promise<UnlistenFn> {
  return listen("segment-ready", (e) => cb(e.payload as never));
}

export function onDialectDetected(
  cb: (p: { dialect: string; confidence: number; source: string }) => void,
): Promise<UnlistenFn> {
  return listen("dialect-detected", (e) => cb(e.payload as never));
}

export function onCodeSwitchDetected(
  cb: (p: { offset: number; from: string; to: string }) => void,
): Promise<UnlistenFn> {
  return listen("code-switch-detected", (e) => cb(e.payload as never));
}

export function onTranslationComplete(
  cb: (p: { total_segments: number }) => void,
): Promise<UnlistenFn> {
  return listen("translation-complete", (e) => cb(e.payload as never));
}

export function onTranslationError(
  cb: (p: { error: string }) => void,
): Promise<UnlistenFn> {
  return listen("translation-error", (e) => cb(e.payload as never));
}
