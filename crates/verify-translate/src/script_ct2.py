"""ctranslate2 NLLB-200 worker.

Same JSON-over-stdio protocol as ``script.py`` (the transformers
fallback). On cache miss, this script also runs the one-time HF →
CTranslate2 model conversion before performing inference.

Inputs (stdin JSON):
    {
        "text": "...",
        "src": "ara_Arab",
        "tgt": "eng_Latn",
        "model": "facebook/nllb-200-distilled-600M",
        "ct2_dir": "/abs/path/to/ct2/model"
    }

Output (stdout JSON):
    {"text": "..."}    on success
    {"error": "..."}   on failure (script exits non-zero)
"""

import json
import os
import sys
from pathlib import Path


def main() -> int:
    try:
        req = json.loads(sys.stdin.read())
        text = req["text"]
        src = req["src"]
        tgt = req["tgt"]
        model_id = req.get("model", "facebook/nllb-200-distilled-600M")
        ct2_dir = Path(req["ct2_dir"])
    except Exception as e:
        sys.stdout.write(json.dumps({"error": f"bad request: {e}"}))
        return 2

    cache = os.environ.get("VERIFY_HF_CACHE")
    if cache:
        os.environ.setdefault("HF_HOME", cache)
        os.environ.setdefault("TRANSFORMERS_CACHE", cache)

    try:
        import ctranslate2  # noqa: F401
    except ImportError as e:
        sys.stdout.write(json.dumps({
            "error": f"ctranslate2 not importable ({e}). "
                     "Install with: pip3 install --user ctranslate2"
        }))
        return 3

    try:
        from transformers import AutoTokenizer
    except ImportError as e:
        sys.stdout.write(json.dumps({
            "error": f"transformers not importable ({e}). "
                     "Install with: pip3 install --user transformers sentencepiece"
        }))
        return 3

    # One-time conversion if the CT2 model dir is missing or empty.
    needs_convert = (not ct2_dir.exists()) or (
        ct2_dir.is_dir() and not any(ct2_dir.iterdir())
    )
    if needs_convert:
        try:
            from ctranslate2.converters import TransformersConverter
        except ImportError as e:
            sys.stdout.write(json.dumps({
                "error": f"ctranslate2 converters unavailable ({e})"
            }))
            return 3
        try:
            ct2_dir.parent.mkdir(parents=True, exist_ok=True)
            converter = TransformersConverter(model_id)
            converter.convert(
                str(ct2_dir),
                quantization="int8",
                force=True,
            )
        except Exception as e:
            sys.stdout.write(json.dumps({
                "error": f"ctranslate2 conversion of {model_id} failed: {e}"
            }))
            return 4

    try:
        import ctranslate2
        translator = ctranslate2.Translator(str(ct2_dir), device="cpu")
        tokenizer = AutoTokenizer.from_pretrained(model_id, src_lang=src)
        encoded = tokenizer.convert_ids_to_tokens(
            tokenizer(text, truncation=True, max_length=512)["input_ids"]
        )
        results = translator.translate_batch(
            [encoded],
            target_prefix=[[tgt]],
            beam_size=1,
            max_decoding_length=512,
        )
        target_tokens = results[0].hypotheses[0]
        # Strip the leading target-language token if present.
        if target_tokens and target_tokens[0] == tgt:
            target_tokens = target_tokens[1:]
        decoded = tokenizer.decode(
            tokenizer.convert_tokens_to_ids(target_tokens),
            skip_special_tokens=True,
        )
    except Exception as e:
        sys.stdout.write(json.dumps({"error": f"ct2 inference failed: {e}"}))
        return 5

    sys.stdout.write(json.dumps({"text": decoded}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
