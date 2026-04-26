#!/usr/bin/env python3
"""SeamlessM4T inference worker for AUGUR.

Sprint 10 P3. Same shape as the NLLB workers — JSON request on
stdin, JSON {"translation": "...", "error": "..."} on stdout.
The `model_dir` field, when supplied, points at the HF cache
populated by `augur install full`.

Tasks:
  translate_text          — text → text translation
  transcribe_translate    — audio path → translated transcript
"""
import json
import sys
import os


def _load(model_dir, model_id):
    if model_dir:
        os.environ.setdefault("HF_HOME", model_dir)
        os.environ.setdefault("TRANSFORMERS_CACHE", model_dir)
    from transformers import AutoProcessor, SeamlessM4Tv2Model

    processor = AutoProcessor.from_pretrained(model_id)
    model = SeamlessM4Tv2Model.from_pretrained(model_id)
    return processor, model


def _translate_text(processor, model, text, src_lang, tgt_lang):
    inputs = processor(text=text, src_lang=src_lang, return_tensors="pt")
    output = model.generate(
        **inputs,
        tgt_lang=tgt_lang,
        generate_speech=False,
    )
    token_ids = output[0].tolist()[0] if hasattr(output[0], "tolist") else output[0]
    return processor.decode(token_ids, skip_special_tokens=True)


def _transcribe_translate(processor, model, audio_path, tgt_lang):
    import torchaudio

    audio, sample_rate = torchaudio.load(audio_path)
    if sample_rate != 16000:
        resampler = torchaudio.transforms.Resample(sample_rate, 16000)
        audio = resampler(audio)
    audio = audio.squeeze()
    inputs = processor(
        audios=audio,
        return_tensors="pt",
        sampling_rate=16000,
    )
    output = model.generate(
        **inputs,
        tgt_lang=tgt_lang,
        generate_speech=False,
    )
    token_ids = output[0].tolist()[0] if hasattr(output[0], "tolist") else output[0]
    return processor.decode(token_ids, skip_special_tokens=True)


def main():
    request = json.loads(sys.stdin.read())
    try:
        processor, model = _load(request.get("model_dir"), request["model_id"])
        task = request.get("task", "translate_text")
        if task == "translate_text":
            translation = _translate_text(
                processor,
                model,
                request["text"],
                request["src_lang"],
                request["tgt_lang"],
            )
        elif task == "transcribe_translate":
            translation = _transcribe_translate(
                processor,
                model,
                request["audio_path"],
                request["tgt_lang"],
            )
        else:
            print(json.dumps({"error": f"unknown task: {task}"}))
            return
        print(json.dumps({"translation": translation}))
    except Exception as exc:  # noqa: BLE001 — worker boundary
        print(json.dumps({"error": f"{type(exc).__name__}: {exc}"}))


if __name__ == "__main__":
    main()
