import type { Language } from "../types";

// Sprint 12 P2 — full 47-language catalog. Three tiers:
//   High quality (22) / Forensic priority (8) / Limited quality (7)
// Tiers map to NLLB / Whisper coverage classes; the `sub` field
// holds dialect or speaker-count notes the picker renders inline.
export const ALL_LANGUAGES: Language[] = [
  // ── High quality ──────────────────────────────────────────
  { code: "ar", name: "Arabic", flag: "🇸🇦", quality: "hi", tier: "High quality", sub: "200M+ speakers" },
  { code: "zh", name: "Chinese (Simplified)", flag: "🇨🇳", quality: "hi", tier: "High quality", sub: "Mandarin" },
  { code: "ru", name: "Russian", flag: "🇷🇺", quality: "hi", tier: "High quality" },
  { code: "fr", name: "French", flag: "🇫🇷", quality: "hi", tier: "High quality" },
  { code: "de", name: "German", flag: "🇩🇪", quality: "hi", tier: "High quality" },
  { code: "es", name: "Spanish", flag: "🇪🇸", quality: "hi", tier: "High quality" },
  { code: "fa", name: "Farsi / Persian", flag: "🇮🇷", quality: "hi", tier: "High quality" },
  { code: "hi", name: "Hindi", flag: "🇮🇳", quality: "hi", tier: "High quality" },
  { code: "ur", name: "Urdu", flag: "🇵🇰", quality: "hi", tier: "High quality" },
  { code: "ko", name: "Korean", flag: "🇰🇵", quality: "hi", tier: "High quality" },
  { code: "ja", name: "Japanese", flag: "🇯🇵", quality: "hi", tier: "High quality" },
  { code: "tr", name: "Turkish", flag: "🇹🇷", quality: "hi", tier: "High quality" },
  { code: "vi", name: "Vietnamese", flag: "🇻🇳", quality: "hi", tier: "High quality" },
  { code: "id", name: "Indonesian", flag: "🇮🇩", quality: "hi", tier: "High quality" },
  { code: "pt", name: "Portuguese", flag: "🇵🇹", quality: "hi", tier: "High quality" },
  { code: "it", name: "Italian", flag: "🇮🇹", quality: "hi", tier: "High quality" },
  { code: "nl", name: "Dutch", flag: "🇳🇱", quality: "hi", tier: "High quality" },
  { code: "pl", name: "Polish", flag: "🇵🇱", quality: "hi", tier: "High quality" },
  { code: "uk", name: "Ukrainian", flag: "🇺🇦", quality: "hi", tier: "High quality" },
  { code: "so", name: "Somali", flag: "🇸🇴", quality: "hi", tier: "High quality" },
  { code: "bn", name: "Bengali", flag: "🇧🇩", quality: "hi", tier: "High quality" },
  { code: "en", name: "English", flag: "🇺🇸", quality: "hi", tier: "High quality" },

  // ── Forensic priority ─────────────────────────────────────
  { code: "ps", name: "Pashto", flag: "🇦🇫", quality: "med", tier: "Forensic priority", sub: "fine-tune available" },
  { code: "prs", name: "Dari", flag: "🇦🇫", quality: "med", tier: "Forensic priority", sub: "Afghan Persian" },
  { code: "am", name: "Amharic", flag: "🇪🇹", quality: "med", tier: "Forensic priority" },
  { code: "ti", name: "Tigrinya", flag: "🇪🇷", quality: "med", tier: "Forensic priority", sub: "Eritrea / Ethiopia" },
  { code: "sw", name: "Swahili", flag: "🇹🇿", quality: "med", tier: "Forensic priority" },
  { code: "th", name: "Thai", flag: "🇹🇭", quality: "med", tier: "Forensic priority" },
  { code: "pa", name: "Panjabi", flag: "🇮🇳", quality: "med", tier: "Forensic priority" },
  { code: "ms", name: "Malay", flag: "🇲🇾", quality: "med", tier: "Forensic priority" },

  // ── Limited quality ───────────────────────────────────────
  { code: "ha", name: "Hausa", flag: "🇳🇬", quality: "low", tier: "Limited quality", sub: "West Africa" },
  { code: "yo", name: "Yoruba", flag: "🇳🇬", quality: "low", tier: "Limited quality" },
  { code: "ig", name: "Igbo", flag: "🇳🇬", quality: "low", tier: "Limited quality" },
  { code: "my", name: "Burmese", flag: "🇲🇲", quality: "low", tier: "Limited quality" },
  { code: "km", name: "Khmer", flag: "🇰🇭", quality: "low", tier: "Limited quality" },
  { code: "lo", name: "Lao", flag: "🇱🇦", quality: "low", tier: "Limited quality" },
  { code: "sd", name: "Sindhi", flag: "🇵🇰", quality: "low", tier: "Limited quality" },
];

export const RTL_CODES = new Set(["ar", "he", "fa", "ur", "ps", "prs", "yi", "sd"]);

export function isRtl(code: string): boolean {
  return RTL_CODES.has(code);
}
