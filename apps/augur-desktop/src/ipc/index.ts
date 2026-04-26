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

export async function createEvidencePackage(args: {
  inputPath: string;
  targetLang: string;
  caseNumber: string;
  examinerName: string;
  agency: string;
  outputPath: string;
}): Promise<string> {
  return invoke<string>("create_evidence_package", args);
}

export function onPackageFileStart(
  cb: (p: { file: string; input_type: string; index: number; total: number }) => void,
): Promise<UnlistenFn> {
  return listen("package-file-start", (e) => cb(e.payload as never));
}

export function onPackageFileDone(
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
  return listen("package-file-done", (e) => cb(e.payload as never));
}

export function onPackageComplete(
  cb: (p: {
    output_path: string;
    total_files: number;
    translated_files: number;
    errors: number;
    case_number: string;
    examiner: string;
    agency: string;
    size_bytes: number;
  }) => void,
): Promise<UnlistenFn> {
  return listen("package-complete", (e) => cb(e.payload as never));
}

export function onPackageError(
  cb: (p: { message: string }) => void,
): Promise<UnlistenFn> {
  return listen("package-error", (e) => cb(e.payload as never));
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

// ── Case state (Sprint 16 P2) ───────────────────────────────────

export interface CaseStatePayload {
  case_number: string;
  examiner_name: string;
  agency: string;
  recent_files: Array<{
    path: string;
    opened_at: string;
    source_lang: string;
    target_lang: string;
    file_type: string;
  }>;
  last_output_dir: string;
  flagged_segments: Record<string, unknown[]>;
}

export async function getCaseState(): Promise<CaseStatePayload> {
  return invoke<CaseStatePayload>("get_case_state");
}

export async function setCaseInfo(args: {
  caseNumber: string;
  examinerName: string;
  agency: string;
}): Promise<void> {
  return invoke<void>("set_case_info", args);
}

export async function addRecentFile(args: {
  path: string;
  sourceLang: string;
  targetLang: string;
  fileType: string;
}): Promise<void> {
  return invoke<void>("add_recent_file", args);
}

export async function saveSegmentFlags(args: {
  filePath: string;
  flags: unknown[];
}): Promise<void> {
  return invoke<void>("save_segment_flags", args);
}

export async function getSegmentFlags(args: {
  filePath: string;
}): Promise<unknown[]> {
  return invoke<unknown[]>("get_segment_flags", args);
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
