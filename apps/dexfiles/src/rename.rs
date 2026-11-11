//! Batch rename: the new names of several items under one rule (find and replace, add text,
//! number them, change the case). The extension is kept unless the rule says otherwise.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Replace,
    Add,
    Number,
    Case,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Lower,
    Upper,
    /// Every word with a capital.
    Title,
    /// Only the first letter.
    Sentence,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub mode: Mode,
    pub find: String,
    pub replace: String,
    pub match_case: bool,
    /// Find and replace also in the extension.
    pub with_ext: bool,
    pub prefix: String,
    pub suffix: String,
    /// Numbering: the name, with `#` where the number goes (at the end if there is none).
    pub base: String,
    pub start: u32,
    pub digits: usize,
    pub case: Case,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            mode: Mode::Replace,
            find: String::new(),
            replace: String::new(),
            match_case: false,
            with_ext: false,
            prefix: String::new(),
            suffix: String::new(),
            base: String::new(),
            start: 1,
            digits: 2,
            case: Case::Lower,
        }
    }
}

/// Name and extension (with its dot); folders and dot files have none.
fn split(name: &str, is_dir: bool) -> (&str, &str) {
    if is_dir {
        return (name, "");
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn replace_all(text: &str, find: &str, with: &str, match_case: bool) -> String {
    if find.is_empty() {
        return text.to_owned();
    }
    if match_case {
        return text.replace(find, with);
    }
    // Without regard to case: compare lower-cased characters one by one.
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = find.chars().flat_map(char::to_lowercase).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < hay.len() {
        let mut j = i;
        let mut k = 0;
        while k < needle.len() && j < hay.len() {
            let low: Vec<char> = hay[j].to_lowercase().collect();
            if needle[k..].starts_with(&low) {
                k += low.len();
                j += 1;
            } else {
                break;
            }
        }
        if k == needle.len() {
            out.push_str(with);
            i = j;
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

fn change_case(text: &str, case: Case) -> String {
    match case {
        Case::Lower => text.to_lowercase(),
        Case::Upper => text.to_uppercase(),
        Case::Title => {
            let mut out = String::new();
            let mut start = true;
            for c in text.chars() {
                if start {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                start = !c.is_alphanumeric();
            }
            out
        }
        Case::Sentence => {
            let lower = text.to_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

/// The new name of the `index`-th item (counting from 0) of the batch.
pub fn apply(rule: &Rule, name: &str, is_dir: bool, index: usize) -> String {
    let (stem, ext) = split(name, is_dir);
    match rule.mode {
        Mode::Replace => {
            if rule.with_ext {
                replace_all(name, &rule.find, &rule.replace, rule.match_case)
            } else {
                format!(
                    "{}{ext}",
                    replace_all(stem, &rule.find, &rule.replace, rule.match_case)
                )
            }
        }
        Mode::Add => format!("{}{stem}{}{ext}", rule.prefix, rule.suffix),
        Mode::Number => {
            let n = format!(
                "{:0width$}",
                rule.start as usize + index,
                width = rule.digits.clamp(1, 9)
            );
            let base = if rule.base.trim().is_empty() {
                stem.to_owned()
            } else {
                rule.base.clone()
            };
            let new = if base.contains('#') {
                base.replacen('#', &n, 1)
            } else if rule.base.trim().is_empty() {
                format!("{base} {n}")
            } else {
                format!("{base}{n}")
            };
            format!("{new}{ext}")
        }
        Mode::Case => format!("{}{ext}", change_case(stem, rule.case)),
    }
}

/// Why a new name can't be used, if it can't.
pub fn problem(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        Some("name_empty")
    } else if name.contains('/') || name == "." || name == ".." {
        Some("name_invalid")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        let r = Rule {
            find: "IMG".into(),
            replace: "Foto".into(),
            ..Rule::default()
        };
        assert_eq!(apply(&r, "img_001.JPG", false, 0), "Foto_001.JPG");
        let r = Rule {
            mode: Mode::Add,
            prefix: "2026 - ".into(),
            suffix: " (v2)".into(),
            ..Rule::default()
        };
        assert_eq!(
            apply(&r, "informe.pdf", false, 0),
            "2026 - informe (v2).pdf"
        );
        assert_eq!(apply(&r, "carpeta.v1", true, 0), "2026 - carpeta.v1 (v2)");
        let r = Rule {
            mode: Mode::Number,
            base: "Vacaciones #".into(),
            start: 9,
            digits: 3,
            ..Rule::default()
        };
        assert_eq!(apply(&r, "x.jpg", false, 1), "Vacaciones 010.jpg");
        let r = Rule {
            mode: Mode::Number,
            ..Rule::default()
        };
        assert_eq!(apply(&r, "nota.txt", false, 0), "nota 01.txt");
        let r = Rule {
            mode: Mode::Case,
            case: Case::Title,
            ..Rule::default()
        };
        assert_eq!(
            apply(&r, "el ÁRBOL de-la vida.MP3", false, 0),
            "El Árbol De-La Vida.MP3"
        );
        let r = Rule {
            mode: Mode::Case,
            case: Case::Sentence,
            ..Rule::default()
        };
        assert_eq!(apply(&r, "ÑANDÚ AZUL.txt", false, 0), "Ñandú azul.txt");
        assert_eq!(apply(&Rule::default(), ".bashrc", false, 0), ".bashrc");
        assert!(problem("a/b").is_some());
    }
}
