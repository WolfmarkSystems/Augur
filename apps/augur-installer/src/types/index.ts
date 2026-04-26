export type Profile = "minimal" | "standard" | "full";
export type Step = 1 | 2 | 3 | 4;
export type ComponentStatus = "waiting" | "active" | "done" | "error";

export interface InstallComponent {
  id: string;
  name: string;
  description: string;
  sizeDisplay: string;
  sizeBytes: number;
  isBundled: boolean;
  componentType: string;
  downloadUrl?: string | null;
}

export interface ComponentState extends InstallComponent {
  status: ComponentStatus;
  percent: number;
  speedMbps: number;
  etaSeconds: number;
}

export interface DownloadProgressPayload {
  component_id: string;
  bytes_downloaded: number;
  total_bytes: number;
  percent: number;
  speed_mbps: number;
  eta_seconds: number;
}

export interface InstallResult {
  profile: Profile;
  componentCount: number;
  totalBytes: number;
}
