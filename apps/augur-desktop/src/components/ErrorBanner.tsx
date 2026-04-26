export type ErrorBannerType =
  | "cli-not-found"
  | "models-missing"
  | "unsupported-file"
  | "translation-failed";

interface Props {
  type: ErrorBannerType | null;
  message?: string;
  onDismiss: () => void;
  onAction?: () => void;
  actionLabel?: string;
}

const TITLES: Record<ErrorBannerType, string> = {
  "cli-not-found": "AUGUR is not installed",
  "models-missing": "Models not installed",
  "unsupported-file": "Unsupported file type",
  "translation-failed": "Translation failed",
};

const COPY: Record<ErrorBannerType, string> = {
  "cli-not-found":
    "The AUGUR translation engine could not be found on this system. Run the AUGUR Installer or install via `cargo install augur`.",
  "models-missing":
    "One or more AUGUR models required for this operation are missing. Open the Model Manager to install them.",
  "unsupported-file":
    "AUGUR cannot process this file type. Supported: mp3, mp4, wav, m4a, aac, ogg, flac, mov, avi, mkv, webm, pdf, txt, md, png, jpg, jpeg, tiff, srt, vtt.",
  "translation-failed":
    "The translation pipeline returned an error. Check the message below and confirm your models are installed.",
};

export default function ErrorBanner({
  type,
  message,
  onDismiss,
  onAction,
  actionLabel,
}: Props) {
  if (!type) return null;
  return (
    <div className={`error-banner is-${type}`} role="alert">
      <div className="error-banner-icon" aria-hidden="true">⚠</div>
      <div className="error-banner-body">
        <div className="error-banner-title">{TITLES[type]}</div>
        <div className="error-banner-text">{COPY[type]}</div>
        {message && (
          <div className="error-banner-detail">{message}</div>
        )}
      </div>
      <div className="error-banner-actions">
        {onAction && actionLabel && (
          <button
            type="button"
            className="btn btn-primary"
            onClick={onAction}
          >
            {actionLabel}
          </button>
        )}
        <button
          type="button"
          className="error-banner-close"
          onClick={onDismiss}
          aria-label="Dismiss"
        >
          ×
        </button>
      </div>
    </div>
  );
}
