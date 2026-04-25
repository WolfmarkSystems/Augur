"""pyannote.audio speaker-diarization worker for verify-stt.

VERIFY's offline-first invariant: pyannote model weights are
downloaded once into the Hugging Face cache and reused on every
subsequent call. Audio data never leaves the examiner's machine.

Reads a single JSON request from stdin:
    {"audio_path": "...", "hf_token": "...",
     "model": "pyannote/speaker-diarization-3.1"}

Emits a single JSON response on stdout:
    {"segments": [
        {"start_ms": 0, "end_ms": 1500, "speaker": "SPEAKER_00"},
        {"start_ms": 1500, "end_ms": 3000, "speaker": "SPEAKER_01"},
        ...
    ]}

On failure prints {"error": "..."} and exits non-zero.
"""

import json
import os
import sys


def main() -> int:
    try:
        req = json.loads(sys.stdin.read())
        audio_path = req["audio_path"]
        hf_token = req["hf_token"]
        model_id = req.get("model", "pyannote/speaker-diarization-3.1")
    except Exception as e:
        sys.stdout.write(json.dumps({"error": f"bad request: {e}"}))
        return 2

    cache = os.environ.get("VERIFY_HF_CACHE")
    if cache:
        os.environ.setdefault("HF_HOME", cache)
        os.environ.setdefault("PYANNOTE_CACHE", cache)

    try:
        from pyannote.audio import Pipeline
    except ImportError as e:
        sys.stdout.write(json.dumps({
            "error": (
                f"pyannote.audio not importable ({e}). "
                "Install with: pip3 install --user pyannote.audio"
            )
        }))
        return 3

    try:
        pipeline = Pipeline.from_pretrained(model_id, use_auth_token=hf_token)
    except Exception as e:
        sys.stdout.write(json.dumps({
            "error": (
                f"pyannote pipeline init failed ({e}). Check the HF token "
                "and that the user has accepted the model's gated terms at "
                f"https://huggingface.co/{model_id}"
            )
        }))
        return 4

    try:
        diarization = pipeline(audio_path)
    except Exception as e:
        sys.stdout.write(json.dumps({"error": f"diarization failed: {e}"}))
        return 5

    segments = []
    for turn, _, speaker in diarization.itertracks(yield_label=True):
        segments.append({
            "start_ms": int(turn.start * 1000),
            "end_ms": int(turn.end * 1000),
            "speaker": str(speaker),
        })

    sys.stdout.write(json.dumps({"segments": segments}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
