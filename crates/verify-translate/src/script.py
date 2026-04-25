"""NLLB-200 offline translation worker invoked by verify-translate.

VERIFY's offline-first invariant: model weights are downloaded once
into the Hugging Face cache by transformers itself; no audio, image,
or text content ever leaves the examiner's machine.

Reads a single JSON request from stdin:
    {"text": "...", "src": "ara_Arab", "tgt": "eng_Latn", "model": "facebook/nllb-200-distilled-600M"}
Emits a single JSON response on stdout:
    {"text": "..."}

Errors print a JSON object with key "error" then exit non-zero.
"""

import json
import os
import sys


def main() -> int:
    try:
        raw = sys.stdin.read()
        req = json.loads(raw)
        text = req["text"]
        src = req["src"]
        tgt = req["tgt"]
        model_id = req.get("model", "facebook/nllb-200-distilled-600M")
    except Exception as e:
        sys.stdout.write(json.dumps({"error": f"bad request: {e}"}))
        return 2

    # Force the HF cache under VERIFY_HF_CACHE if set so all VERIFY
    # weights land under one auditable directory.
    cache = os.environ.get("VERIFY_HF_CACHE")
    if cache:
        os.environ.setdefault("HF_HOME", cache)
        os.environ.setdefault("TRANSFORMERS_CACHE", cache)

    try:
        from transformers import AutoModelForSeq2SeqLM, AutoTokenizer
    except ImportError as e:
        sys.stdout.write(json.dumps({
            "error": (
                f"transformers not importable ({e}). "
                "Install with: pip3 install --user transformers torch sentencepiece"
            )
        }))
        return 3

    try:
        tokenizer = AutoTokenizer.from_pretrained(model_id, src_lang=src)
        model = AutoModelForSeq2SeqLM.from_pretrained(model_id)
        inputs = tokenizer(text, return_tensors="pt", truncation=True, max_length=512)
        forced_bos = tokenizer.convert_tokens_to_ids(tgt)
        out = model.generate(
            **inputs,
            forced_bos_token_id=forced_bos,
            max_length=512,
            num_beams=1,
        )
        decoded = tokenizer.batch_decode(out, skip_special_tokens=True)[0]
    except Exception as e:
        sys.stdout.write(json.dumps({"error": f"inference failed: {e}"}))
        return 4

    sys.stdout.write(json.dumps({"text": decoded}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
