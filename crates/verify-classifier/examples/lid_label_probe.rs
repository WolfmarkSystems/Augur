use fasttext::FastText;
use std::path::PathBuf;

fn main() {
    let home = std::env::var("HOME").expect("HOME");
    let model_path = PathBuf::from(home).join(".cache/verify/models/lid.176.ftz");
    println!("Loading {}", model_path.display());
    let model = match FastText::load_model(&model_path) {
        Ok(m) => m,
        Err(e) => {
            println!("LOAD ERROR: {e}");
            std::process::exit(1);
        }
    };
    println!("Loaded.");
    let cases: &[(&str, &str)] = &[
        ("Arabic",   "مرحبا بالعالم، كيف حالك اليوم؟"),
        ("Chinese",  "你好,世界,你今天怎么样?"),
        ("Russian",  "Привет мир, как у тебя сегодня дела?"),
        ("Spanish",  "Hola mundo, ¿cómo estás hoy?"),
        ("English",  "The quick brown fox jumps over the lazy dog."),
        ("Japanese", "こんにちは世界、今日はどうですか"),
        ("Korean",   "안녕하세요 세계, 오늘 어떻게 지내세요"),
        ("German",   "Hallo Welt, wie geht es dir heute"),
    ];
    for (label, text) in cases {
        let preds = model.predict(text, 5, 0.0);
        println!("\n{} input: {:?}", label, text);
        if preds.is_empty() {
            println!("  (no predictions)");
        }
        for p in preds.iter().take(5) {
            println!("  raw_label={:?} prob={:.4}", p.label, p.prob);
        }
    }
}
