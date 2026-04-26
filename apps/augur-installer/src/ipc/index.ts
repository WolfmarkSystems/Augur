import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DownloadProgressPayload,
  InstallComponent,
  Profile,
} from "../types";

export async function getProfileComponents(
  profile: Profile,
): Promise<InstallComponent[]> {
  return invoke<InstallComponent[]>("get_profile_components", { profile });
}

export async function getTotalSize(profile: Profile): Promise<number> {
  return invoke<number>("get_total_size", { profile });
}

export async function startInstallation(profile: Profile): Promise<void> {
  return invoke<void>("start_installation", { profile });
}

export async function checkExistingInstallation(): Promise<unknown> {
  return invoke<unknown>("check_existing_installation");
}

export async function launchAugur(): Promise<void> {
  return invoke<void>("launch_augur");
}

// ── Event listeners ─────────────────────────────────────────────

export function onComponentStart(
  cb: (payload: { id: string; index: number; total: number }) => void,
): Promise<UnlistenFn> {
  return listen("install-component-start", (e) =>
    cb(e.payload as { id: string; index: number; total: number }),
  );
}

export function onDownloadProgress(
  cb: (payload: DownloadProgressPayload) => void,
): Promise<UnlistenFn> {
  return listen("download-progress", (e) =>
    cb(e.payload as DownloadProgressPayload),
  );
}

export function onComponentDone(
  cb: (payload: { id: string; index: number }) => void,
): Promise<UnlistenFn> {
  return listen("install-component-done", (e) =>
    cb(e.payload as { id: string; index: number }),
  );
}

export function onComponentError(
  cb: (payload: { id: string; index: number; error: string }) => void,
): Promise<UnlistenFn> {
  return listen("install-component-error", (e) =>
    cb(e.payload as { id: string; index: number; error: string }),
  );
}

export function onInstallComplete(cb: () => void): Promise<UnlistenFn> {
  return listen("install-complete", () => cb());
}
