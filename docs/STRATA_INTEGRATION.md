# VERIFY — Strata Plugin Integration

How to wire VERIFY into the Strata plugin grid as a first-class
analyzer that surfaces foreign-language translations alongside
the Strata-native forensic plugins.

## What VERIFY contributes

VERIFY classifies foreign-language content in evidence and
emits **machine-translated** artifacts that surface in Strata's
Communications panel. Every artifact is advisory — Strata's UI
surfaces the same `[MT — review by certified human translator]`
prefix and `is_machine_translation = true` raw_data flag at all
export paths.

## Build

VERIFY's Strata trait impl is feature-gated so the default
build stays small. To produce a Strata-compatible build:

```bash
cargo build --features verify-plugin-sdk/strata --release
```

The vendored Strata SDK lives at
`vendor/strata-plugin-sdk/`; a minimal `vendor/strata-fs/` stub
satisfies the upstream SDK's `VirtualFilesystem` re-export so
the analyzer compiles without pulling in NTFS/APFS/ext4/EWF
parsers VERIFY does not need.

## Wiring into Strata's runtime

1. Add VERIFY's plugin SDK to Strata's `Cargo.toml`:
   ```toml
   verify-plugin-sdk = {
       path = "../verify/crates/verify-plugin-sdk",
       features = ["strata"],
   }
   ```

2. Register VERIFY in Strata's plugin grid (`PLUGIN_NAMES`):
   ```rust
   pub const PLUGIN_NAMES: &[&str] = &[
       // existing entries...
       "VERIFY",
   ];
   ```

3. Surface VERIFY's metadata in the Strata UI
   (`types/index.ts`):
   ```typescript
   {
     name: "VERIFY",
     version: "1.0.0",
     category: "Analyzer",
     description: "Foreign language detection and translation",
   }
   ```

4. Wire the dispatch in `run_plugin`:
   ```rust
   "VERIFY" => {
       let plugin = verify_plugin_sdk::VerifyStrataPlugin::new();
       plugin.execute(context)
   }
   ```

## Artifact shape

Every translation artifact emitted by VERIFY:

| field          | value                                                              |
| -------------- | ------------------------------------------------------------------ |
| `category`     | `ArtifactCategory::Communications`                                 |
| `subcategory`  | `"Foreign Language Translation"`                                   |
| `title`        | `[MT — review by a certified human translator] VERIFY Translation: <file>` |
| `detail`       | The translated text                                                |
| `forensic_value` | `ForensicValue::High`                                            |
| `confidence`   | `50` (Medium — MT output)                                          |
| `raw_data`     | JSON with `source_text`, `translated_text`, `source_language`, `target_language`, `model`, `is_machine_translation: true`, `advisory_notice`, `segments` |

The advisory survives in two places:

1. The artifact `title` always prefixes
   `[MT — review by a certified human translator]`.
2. `raw_data.advisory_notice` and
   `raw_data.is_machine_translation` are always populated.

`assert_advisory_invariant` enforces both before the artifact
leaves `walk_and_translate`. If either is stripped (by a
modification or a serialization bug), the trait `execute()`
call fails with `PluginError::Internal` rather than emit an
unlabeled translation.

## Testing the integration

The default-build test suite covers the adapter shape via
unit tests under `crates/verify-plugin-sdk/src/strata_impl.rs`.
A live integration test walks a temp evidence directory and
asserts the advisory invariant holds:

```bash
VERIFY_RUN_INTEGRATION_TESTS=1 cargo test \
    --features verify-plugin-sdk/strata \
    -- --include-ignored strata_plugin_processes_real_arabic_evidence
```

For metadata sanity:

```bash
cargo test --features verify-plugin-sdk/strata \
    strata_plugin_metadata_complete
```

## Forensic discipline

- The MT advisory is non-suppressible. There is no flag, env
  var, or build option that turns it off.
- Speaker-diarization labels (when `--diarize` is used at the
  CLI layer) are accompanied by the SPEAKER advisory; in
  Strata mode the diarization path isn't surfaced today —
  audio inputs produce flat translations only.
- Subtitle (`.srt` / `.vtt`) inputs are skipped by the Strata
  walker; analysts who want subtitle translations should run
  `verify translate --output-srt` directly.
