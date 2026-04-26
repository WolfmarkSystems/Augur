#!/usr/bin/env python3
"""CAMeL Tools Arabic dialect identification worker for AUGUR.

Sprint 10 P4. JSON in, JSON out.

Request:  {"text": "..."}
Response: {"dialect": "EGY", "confidence": 0.89}
       or {"error": "..."}
"""
import json
import sys


def _identify(text: str):
    from camel_tools.dialectid import DialectIdentifier

    did = DialectIdentifier.pretrained()
    result = did.predict(text)
    # camel-tools >= 1.5 returns a list of dicts with .top / .scores
    # — fall back to a defensive shape probe to stay version-tolerant.
    top = getattr(result, "top", None) or result[0]["top"]
    scores = getattr(result, "scores", None) or result[0]["scores"]
    confidence = float(scores.get(top, 0.0)) if isinstance(scores, dict) else 0.0
    return top, confidence


def main():
    request = json.loads(sys.stdin.read())
    try:
        dialect, confidence = _identify(request["text"])
        print(json.dumps({"dialect": dialect, "confidence": confidence}))
    except Exception as exc:  # noqa: BLE001 — worker boundary
        print(json.dumps({"error": f"{type(exc).__name__}: {exc}"}))


if __name__ == "__main__":
    main()
