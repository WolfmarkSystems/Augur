# AUGUR — Known Language Detection Limitations

This document enumerates language-pair confusions and other
classifier limitations AUGUR ships with. None of these are bugs;
they are inherent to the underlying open-source models. The
intent of this document is to make sure examiners working high-
stakes cases know when to bring a human linguist into the loop.

## Pashto / Persian (Farsi) Confusion

**Languages affected:** Pashto (`ps`), Persian / Farsi (`fa`)
**Severity:** High for LE / IC casework in Afghanistan and
Pakistan.

**Root cause:** Both languages use the Arabic script with
overlapping character distributions and a substantial shared
vocabulary. The current open-source LID models — `whichlang`
(embedded weights, 16-language) and Meta's `lid.176.ftz`
(176 languages, used via `fasttext-pure-rs`) — were not
specifically optimized for the Pashto/Persian distinction. The
Sprint 5 P1 probe confirmed this: a clearly Pashto sentence
classifies as `fa` with confidence ≈ 0.76.

**Behavior:** AUGUR may classify Pashto text as `fa`. The
confidence score does NOT reliably distinguish "really Persian"
from "Pashto misclassified as Persian"; both routinely sit in
the 0.7 – 0.95 band.

**Mitigation in AUGUR:**
1. When the source language is detected as `fa`, the translation
   pipeline appends a disambiguation note to the
   `TranslationResult.advisory_notice` (see
   `crates/augur-translate/src/lib.rs`). The mandatory
   machine-translation advisory is preserved alongside.
2. The confidence-tier system (Sprint 6 P2) flags inputs under
   10 words as `LOW` with a "verify with a human linguist"
   advisory regardless of the raw model score.

**Examiner action:** When `fa` is reported in evidence from a
geographic / contextual setting where Pashto is plausible
(Afghanistan, Pakistan tribal areas, Pashtun diaspora
communications), always verify with a certified human linguist
who reads both languages fluently. Do NOT rely on confidence
score alone.

**Sprint reference:** AUGUR Sprint 5 (2026-04-25) confirmed via
`crates/augur-classifier/examples/lid_pure_probe.rs`. Sprint 6
(2026-04-26) wired the disambiguation advisory.

## Short-Text Classification

**Input length:** < 10 words.
**Severity:** Medium — affects all language pairs equally.
**Behavior:** Every LID model degrades on very short inputs.
Whichlang in particular reports `1.0` on inputs as small as one
word, which is misleading. AUGUR's Sprint 6 P2 confidence-tier
system surfaces a `LOW` tier with a "Short input (N words) —
language detection may be unreliable" advisory whenever the
input is below `SHORT_INPUT_WORD_COUNT = 10`, regardless of the
raw model score.

## Other Forensically Important Language Pairs

The following pairs are also commonly confused by automated
language identification. AUGUR does not surface specific
advisories for these today (Sprint 7+ candidates), but examiners
working in these contexts should treat any single-pair
classification as advisory:

| Pair | ISO codes | Notes |
| ---- | --------- | ----- |
| Serbian / Croatian / Bosnian | `sr` / `hr` / `bs` | South Slavic, Cyrillic vs Latin script overlap |
| Malay / Indonesian | `ms` / `id` | Lexically very close, different geographic contexts |
| Hindi / Urdu | `hi` / `ur` | Same spoken language, different scripts (Devanagari vs Nasta'liq) |
| Swahili / Comorian | `sw` / various | Bantu family, regional spread |
| Punjabi (Eastern / Western) | `pa` / `pnb` | Different scripts (Gurmukhi vs Shahmukhi) |

When any of these languages are detected on high-stakes
casework, verify with a human linguist before treating the
classification as definitive.

## Reading the Confidence Tier

AUGUR emits `HIGH`, `MEDIUM`, or `LOW` for every classification
(Sprint 6 P2):

| Tier | Score | Word count | Examiner action |
| ---- | ----- | ---------- | --------------- |
| `HIGH` | ≥ 0.85 | ≥ 10 | Use directly for casework — model is well within its trained envelope. |
| `MEDIUM` | 0.60 – 0.85 | ≥ 10 | Likely correct; verify with a human linguist if the result is critical. |
| `LOW` | < 0.60, OR < 10 words | any | Uncertain. Human review recommended. |

The tier is encoded in the batch JSON / CSV per file
(`confidence_tier`) and printed alongside every classify /
translate output line.

## What AUGUR Does NOT Decide For You

AUGUR surfaces the model's best guess plus the right caveats —
it does not decide whether a piece of evidence is admissible,
whether a translation is accurate enough for court, or whether
the speaker / writer is who you think they are. Those calls
belong to humans. The mandatory machine-translation advisory
that fires on every translation output is part of this discipline:
it is forensic-tool hygiene, not a UX flourish, and it is not
suppressible by any flag.
