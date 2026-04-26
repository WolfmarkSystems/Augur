"""Sprint 4 P4 — ctranslate2 vs transformers benchmark.

Driven by the same JSON-over-stdio worker scripts the production
augur-translate engine uses, so the numbers reflect what
forensic users will actually see.

Usage:
    python3 tests/run_benchmark.py path/to/fixture.txt
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SCRIPT_TF = ROOT / "crates" / "augur-translate" / "src" / "script.py"
SCRIPT_CT2 = ROOT / "crates" / "augur-translate" / "src" / "script_ct2.py"
HF_CACHE = Path.home() / ".cache" / "verify" / "models" / "nllb"
CT2_DIR = HF_CACHE / "ct2"
ENV = {**os.environ, "AUGUR_HF_CACHE": str(HF_CACHE)}


def time_call(label: str, script: Path, request: dict) -> tuple[float, str]:
    proc = subprocess.run(
        ["python3", "-c", script.read_text()],
        input=json.dumps(request),
        text=True,
        capture_output=True,
        env=ENV,
    )
    dur = -1.0
    out = proc.stdout.strip()
    err = proc.stderr.strip()
    if proc.returncode != 0:
        return (dur, f"FAIL exit={proc.returncode} stderr={err[:300]}")
    try:
        resp = json.loads(out)
    except Exception as e:
        return (dur, f"FAIL parse={e} stdout={out[:300]}")
    if "error" in resp:
        return (dur, f"FAIL worker={resp['error']}")
    return (dur, resp.get("text", ""))


def run_one(label: str, script: Path, request: dict) -> None:
    print(f"\n=== {label} ===")
    start = time.perf_counter()
    proc = subprocess.run(
        ["python3", "-c", script.read_text()],
        input=json.dumps(request),
        text=True,
        capture_output=True,
        env=ENV,
    )
    elapsed = time.perf_counter() - start
    if proc.returncode != 0:
        print(f"  FAIL exit={proc.returncode}")
        if proc.stderr.strip():
            print(f"  stderr: {proc.stderr.strip()[:500]}")
        if proc.stdout.strip():
            print(f"  stdout: {proc.stdout.strip()[:500]}")
        return
    out = proc.stdout.strip()
    try:
        resp = json.loads(out)
    except Exception as e:
        print(f"  FAIL parse={e}")
        print(f"  stdout: {out[:500]}")
        return
    if "error" in resp:
        print(f"  FAIL worker error: {resp['error'][:500]}")
        return
    text = resp.get("text", "")
    print(f"  elapsed: {elapsed:.2f}s")
    print(f"  output:  {text[:200]}")
    print(f"  ELAPSED:{elapsed:.4f}")


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: run_benchmark.py <fixture.txt>")
        return 2
    fixture = Path(sys.argv[1]).read_text().strip()
    print(f"fixture: {len(fixture.split())} words, {len(fixture)} chars")
    print(f"hf_cache: {HF_CACHE}")
    print(f"ct2_dir:  {CT2_DIR}")

    base_request = {
        "text": fixture,
        "src": "ara_Arab",
        "tgt": "eng_Latn",
        "model": "facebook/nllb-200-distilled-600M",
    }

    # Warm-up: ensures the model is downloaded + into the page cache.
    print("\n--- WARMUP (transformers) ---")
    run_one("warmup-tf", SCRIPT_TF, base_request)

    # Timed transformers run.
    run_one("transformers", SCRIPT_TF, base_request)

    # ctranslate2 run (first call also runs the conversion).
    ct2_request = {**base_request, "ct2_dir": str(CT2_DIR)}
    print("\n--- WARMUP (ct2 — also runs conversion if first time) ---")
    run_one("warmup-ct2", SCRIPT_CT2, ct2_request)
    run_one("ctranslate2", SCRIPT_CT2, ct2_request)
    return 0


if __name__ == "__main__":
    sys.exit(main())
