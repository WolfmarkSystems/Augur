//! Sprint 5 P1 — fasttext-pure-rs probe.
//!
//! Tests whether `fasttext-pure-rs = 0.1.0` reads Meta's
//! `lid.176.ftz` correctly. The Sprint 1 probe confirmed
//! `fasttext = 0.8.0` does NOT (Arabic → Esperanto). If this
//! probe shows the pure-Rust crate produces sane labels for
//! Arabic / Chinese / Russian / Spanish, we re-promote the
//! 176-language fastText backend; otherwise whichlang remains
//! the production ceiling.
//!
//! Reproduce:
//!   cargo run -p verify-classifier --features fasttext-probe \
//!       --example lid_pure_probe
//!
//! Pass criteria (per Sprint 5 spec):
//!   Arabic  → __label__ar (not __label__eo)
//!   Chinese → __label__zh (not __label__sr)
//!   Russian → __label__ru (not __label__ar)
//!   Spanish → __label__es (not __label__en)

use fasttext_pure_rs::FastText;
use std::path::PathBuf;

fn main() {
    let home = std::env::var("HOME").expect("HOME");
    let model = PathBuf::from(home).join(".cache/verify/models/lid.176.ftz");
    if !model.exists() {
        eprintln!("lid.176.ftz not found at {model:?}; run `verify classify` once to seed the cache.");
        std::process::exit(2);
    }

    let ft = FastText::load(&model).expect("FastText::load");
    let cases: &[(&str, &str, &str)] = &[
        ("ar", "مرحبا بالعالم، كيف حالك اليوم؟", "Arabic"),
        ("zh", "你好世界,今天天气真好。", "Chinese"),
        ("ru", "Привет мир, как ваши дела сегодня?", "Russian"),
        ("es", "Hola mundo, ¿cómo estás hoy?", "Spanish"),
        ("fa", "سلام دنیا، حال شما چطور است؟", "Persian/Farsi"),
        ("ps", "سلام نړۍ، تاسو څنګه یاست؟", "Pashto"),
        ("ur", "ہیلو ورلڈ، آج آپ کیسے ہیں؟", "Urdu"),
    ];

    let mut all_ok = true;
    for (expected, sample, name) in cases {
        let preds = ft
            .predict(sample, 1, 0.0)
            .expect("predict");
        let top = preds.first().expect("at least one prediction");
        let label = top
            .label
            .strip_prefix("__label__")
            .unwrap_or(top.label.as_str());
        let pass = label == *expected;
        all_ok &= pass;
        println!(
            "{:>14}: expected={:<3} got={:<5} prob={:.3}  {}",
            name,
            expected,
            label,
            top.probability,
            if pass { "PASS" } else { "FAIL" },
        );
    }
    println!(
        "\nfasttext-pure-rs vs lid.176.ftz: {}",
        if all_ok { "COMPATIBLE" } else { "INCOMPATIBLE" }
    );
    std::process::exit(if all_ok { 0 } else { 1 });
}
