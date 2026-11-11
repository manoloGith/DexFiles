//! Interface text in English and Spanish. The language follows `LANG` (Android's system
//! language, which craft-android puts there) unless the settings choose one.

use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0);
const EN: u8 = 0;
const ES: u8 = 1;

pub fn set_language(choice: crate::settings::Language) {
    let lang = match choice {
        crate::settings::Language::Spanish => ES,
        crate::settings::Language::English => EN,
        crate::settings::Language::Auto => {
            let env = std::env::var("LANG")
                .or_else(|_| std::env::var("LC_ALL"))
                .unwrap_or_default();
            if env.starts_with("es") { ES } else { EN }
        }
    };
    LANG.store(lang, Ordering::Relaxed);
}

pub fn is_spanish() -> bool {
    LANG.load(Ordering::Relaxed) == ES
}

/// Spanish writes 1,5 MB and 31/12/2026.
pub fn decimal_comma() -> bool {
    is_spanish()
}

/// The text for `key` (the key itself if missing, so a gap shows but never breaks).
pub fn t(key: &str) -> &str {
    let es = is_spanish();
    match STRINGS.binary_search_by(|(k, _, _)| k.cmp(&key)) {
        Ok(i) => {
            let (_, en, spanish) = STRINGS[i];
            if es { spanish } else { en }
        }
        Err(_) => key,
    }
}

/// `t(key)` with `{}` replaced by `arg`.
pub fn tf(key: &str, arg: impl std::fmt::Display) -> String {
    t(key).replacen("{}", &arg.to_string(), 1)
}

/// (key, English, Spanish), sorted by key (a test checks).
static STRINGS: &[(&str, &str, &str)] = &include!(concat!(env!("OUT_DIR"), "/strings.in"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        for w in STRINGS.windows(2) {
            assert!(w[0].0 < w[1].0, "{} / {}", w[0].0, w[1].0);
        }
    }

    #[test]
    fn lookup() {
        set_language(crate::settings::Language::Spanish);
        assert_eq!(t("back"), "Atrás");
        assert_eq!(t("no-such-key"), "no-such-key");
        set_language(crate::settings::Language::English);
        assert_eq!(t("back"), "Back");
    }
}
