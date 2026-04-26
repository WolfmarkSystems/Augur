# AUGUR benchmark fixtures

Super Sprint Group C P5. Hand-curated short corpora used by
`augur benchmark` to time classification and translation
pipelines.

| File                  | Language     | Words | Notes                              |
| --------------------- | ------------ | ----- | ---------------------------------- |
| `arabic_short.txt`    | Arabic (MSA) | ~25   | Generic forensic prose             |
| `arabic_medium.txt`   | Arabic (MSA) | ~150  | Full investigation paragraph       |
| `arabic_long.txt`     | Arabic (MSA) | ~450  | 3× concatenation of medium         |
| `mixed_languages.txt` | Arabic + EN  | ~50   | Code-switched content              |
| `pashto_sample.txt`   | Pashto       | ~30   | Pashto-specific glyphs (ډ ښ ګ ړ ۍ) |

The corpora are deterministic — committed to the repo so
benchmark runs across machines compare directly. Do NOT replace
with personal-data examples.
