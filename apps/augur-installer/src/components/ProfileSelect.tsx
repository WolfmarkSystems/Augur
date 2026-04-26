import type { Profile } from "../types";

interface ProfileMeta {
  id: Profile;
  name: string;
  size: string;
  description: string;
  components: string[];
  recommended?: boolean;
}

const PROFILES: ProfileMeta[] = [
  {
    id: "minimal",
    name: "Minimal",
    size: "2.5 GB",
    description: "Basic documents and clear audio.",
    components: ["Whisper Tiny", "NLLB-200 600M", "fastText LID"],
  },
  {
    id: "standard",
    name: "Standard",
    size: "11 GB",
    description: "Recommended for most LE/IC casework.",
    recommended: true,
    components: [
      "Whisper Large-v3",
      "NLLB-200 1.3B",
      "CAMeL Arabic",
      "fastText LID",
    ],
  },
  {
    id: "full",
    name: "Full",
    size: "15 GB",
    description: "All models including Pashto, Dari, SeamlessM4T.",
    components: [
      "All Standard models",
      "Whisper Pashto + Dari",
      "SeamlessM4T",
      "Speaker Diarization",
    ],
  },
];

export default function ProfileSelect({
  selected,
  onSelect,
}: {
  selected: Profile;
  onSelect: (p: Profile) => void;
}) {
  const sel = PROFILES.find((p) => p.id === selected) ?? PROFILES[1];
  return (
    <div className="profile-select">
      <h2 className="screen-title">Choose an install profile</h2>
      <p className="screen-sub">
        You can re-run the installer later to add more components.
      </p>
      <div className="profile-cards">
        {PROFILES.map((p) => (
          <button
            type="button"
            key={p.id}
            className={`profile-card ${selected === p.id ? "is-selected" : ""}`}
            onClick={() => onSelect(p.id)}
            aria-pressed={selected === p.id}
          >
            {p.recommended && (
              <span className="profile-badge">Recommended</span>
            )}
            <div className="profile-name">{p.name}</div>
            <div className="profile-size">{p.size}</div>
            <div className="profile-desc">{p.description}</div>
          </button>
        ))}
      </div>
      <div className="profile-preview">
        <div className="profile-preview-title">
          {sel.name} includes:
        </div>
        <ul className="profile-preview-list">
          {sel.components.map((c) => (
            <li key={c}>{c}</li>
          ))}
        </ul>
      </div>
    </div>
  );
}
