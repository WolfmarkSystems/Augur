import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ModelStatus } from "../types";

export async function openEvidenceDialog(): Promise<string | null> {
  return invoke<string | null>("open_evidence_dialog");
}

export async function openDirectoryDialog(): Promise<string | null> {
  return invoke<string | null>("open_directory_dialog");
}

export async function checkAugurAvailable(): Promise<boolean> {
  return invoke<boolean>("check_augur_available");
}

export async function augurBinaryPath(): Promise<string | null> {
  return invoke<string | null>("augur_binary_path");
}

export async function startBatchTranslation(args: {
  inputDir: string;
  targetLang: string;
  outputPath: string;
  format: "html" | "json" | "csv" | "zip";
}): Promise<void> {
  return invoke<void>("start_batch_translation", args);
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

export interface ModelStatusResponse {
  profile: string;
  models: Array<{
    id: string;
    name: string;
    tier: string;
    installed: boolean;
    size_bytes: number;
    size_display: string;
  }>;
  total_installed_bytes: number;
  total_bytes: number;
  profile_complete: boolean;
}

export async function getModelStatus(): Promise<ModelStatusResponse> {
  return invoke<ModelStatusResponse>("get_model_status");
}

export async function installModel(modelId: string): Promise<void> {
  return invoke<void>("install_model", { modelId });
}

export function onModelInstallProgress(
  cb: (p: {
    type: string;
    model_id: string;
    name?: string;
    error?: string;
    [k: string]: unknown;
  }) => void,
): Promise<UnlistenFn> {
  return listen("model-install-progress", (e) => cb(e.payload as never));
}

export function onModelInstallFinished(
  cb: (p: { model_id: string }) => void,
): Promise<UnlistenFn> {
  return listen("model-install-finished", (e) => cb(e.payload as never));
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
  cb: (p: { message?: string; error?: string }) => void,
): Promise<UnlistenFn> {
  return listen("translation-error", (e) => cb(e.payload as never));
}

// ── Batch event listeners ───────────────────────────────────────

export function onBatchFileStart(
  cb: (p: { file: string; input_type: string; index: number; total: number }) => void,
): Promise<UnlistenFn> {
  return listen("batch-file-start", (e) => cb(e.payload as never));
}

export function onBatchFileDone(
  cb: (p: {
    file: string;
    input_type: string;
    detected_language: string;
    is_foreign: boolean;
    translated: boolean;
    error: string | null;
    processed: number;
    total: number;
  }) => void,
): Promise<UnlistenFn> {
  return listen("batch-file-done", (e) => cb(e.payload as never));
}

export function onBatchComplete(
  cb: (p: {
    total_files: number;
    processed: number;
    foreign_files: number;
    translated: number;
    errors: number;
    elapsed_seconds: number;
  }) => void,
): Promise<UnlistenFn> {
  return listen("batch-complete", (e) => cb(e.payload as never));
}

export function onBatchError(
  cb: (p: { message: string }) => void,
): Promise<UnlistenFn> {
  return listen("batch-error", (e) => cb(e.payload as never));
}
