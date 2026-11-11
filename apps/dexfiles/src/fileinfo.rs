//! What kind of file something is (from its extension), and how sizes and dates read.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    Folder,
    Image,
    Video,
    Audio,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Text,
    Code,
    Archive,
    App,
    Other,
}

impl Category {
    /// The categories a search can filter by.
    pub const FILTERS: [Category; 12] = [
        Category::Folder,
        Category::Image,
        Category::Video,
        Category::Audio,
        Category::Pdf,
        Category::Document,
        Category::Spreadsheet,
        Category::Presentation,
        Category::Text,
        Category::Code,
        Category::Archive,
        Category::App,
    ];

    pub fn of(ext: &str, is_dir: bool) -> Category {
        if is_dir {
            return Category::Folder;
        }
        match ext {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "heic"
            | "heif" | "avif" | "svg" | "raw" | "dng" | "cr2" | "nef" => Category::Image,
            "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "3gp" | "wmv" | "flv" | "mpg"
            | "mpeg" | "ts" => Category::Video,
            "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "wma" | "aiff" | "mid"
            | "midi" | "amr" => Category::Audio,
            "pdf" => Category::Pdf,
            "doc" | "docx" | "odt" | "rtf" | "pages" | "epub" | "fodt" => Category::Document,
            "xls" | "xlsx" | "xlsm" | "xlsb" | "ods" | "csv" | "tsv" | "numbers" | "fods" => {
                Category::Spreadsheet
            }
            "ppt" | "pptx" | "odp" | "key" | "fodp" => Category::Presentation,
            "txt" | "md" | "log" | "ini" | "cfg" | "conf" | "nfo" | "srt" | "vtt" | "tex" => {
                Category::Text
            }
            "rs" | "c" | "h" | "cpp" | "hpp" | "py" | "js" | "tsx" | "jsx" | "java" | "kt"
            | "kts" | "go" | "rb" | "php" | "sh" | "bash" | "html" | "htm" | "css" | "json"
            | "xml" | "yaml" | "yml" | "toml" | "sql" | "swift" | "cs" | "lua" | "gradle"
            | "properties" => Category::Code,
            "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "tar.gz" | "tar.bz2"
            | "tar.xz" | "tar.zst" | "zst" | "tzst" | "jar" | "iso" | "cbz" | "cbr" => Category::Archive,
            "apk" | "apks" | "xapk" | "exe" | "msi" | "deb" | "rpm" | "appimage" | "dmg" => {
                Category::App
            }
            _ => Category::Other,
        }
    }

    /// The i18n key of its name.
    pub fn key(self) -> &'static str {
        match self {
            Category::Folder => "cat_folder",
            Category::Image => "cat_image",
            Category::Video => "cat_video",
            Category::Audio => "cat_audio",
            Category::Pdf => "cat_pdf",
            Category::Document => "cat_document",
            Category::Spreadsheet => "cat_spreadsheet",
            Category::Presentation => "cat_presentation",
            Category::Text => "cat_text",
            Category::Code => "cat_code",
            Category::Archive => "cat_archive",
            Category::App => "cat_app",
            Category::Other => "cat_other",
        }
    }

    /// The colour of its icon.
    pub fn color(self) -> egui::Color32 {
        use egui::Color32 as C;
        match self {
            Category::Folder => C::from_rgb(0x3b, 0x9b, 0xf5),
            Category::Image => C::from_rgb(0xaf, 0x52, 0xde),
            Category::Video => C::from_rgb(0xff, 0x5e, 0x57),
            Category::Audio => C::from_rgb(0xff, 0x37, 0x8f),
            Category::Pdf => C::from_rgb(0xe5, 0x39, 0x35),
            Category::Document => C::from_rgb(0x2f, 0x6f, 0xe4),
            Category::Spreadsheet => C::from_rgb(0x1f, 0xa3, 0x5b),
            Category::Presentation => C::from_rgb(0xf5, 0x8a, 0x1f),
            Category::Text => C::from_rgb(0x8e, 0x8e, 0x93),
            Category::Code => C::from_rgb(0x5e, 0x5c, 0xe6),
            Category::Archive => C::from_rgb(0xa2, 0x84, 0x5e),
            Category::App => C::from_rgb(0x30, 0xb0, 0xc7),
            Category::Other => C::from_rgb(0x9a, 0xa0, 0xa6),
        }
    }

    /// Whether its content is plain text (searchable, previewable as text).
    pub fn is_text(self) -> bool {
        matches!(self, Category::Text | Category::Code)
    }
}

/// `1,5 MB`, with the locale's decimal separator.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    #[allow(clippy::cast_precision_loss)] // a size to show
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    let text = if v < 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.0}")
    };
    let text = if crate::i18n::decimal_comma() {
        text.replace('.', ",")
    } else {
        text
    };
    format!("{text} {}", UNITS[unit])
}

/// A modification date for a listing: time today, "Yesterday", or the date.
pub fn format_date(secs: Option<i64>) -> String {
    use chrono::{Local, TimeZone};
    let Some(secs) = secs else {
        return "—".to_owned();
    };
    let Some(t) = Local.timestamp_opt(secs, 0).single() else {
        return "—".to_owned();
    };
    let now = Local::now();
    let today = now.date_naive();
    let day = t.date_naive();
    if day == today {
        format!("{} {}", crate::i18n::t("today"), t.format("%H:%M"))
    } else if today.pred_opt() == Some(day) {
        format!("{} {}", crate::i18n::t("yesterday"), t.format("%H:%M"))
    } else if crate::i18n::decimal_comma() {
        t.format("%d/%m/%Y %H:%M").to_string()
    } else {
        t.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// A full date, for the info panel.
pub fn format_date_long(secs: Option<i64>) -> String {
    use chrono::{Local, TimeZone};
    let Some(t) = secs.and_then(|s| Local.timestamp_opt(s, 0).single()) else {
        return "—".to_owned();
    };
    if crate::i18n::decimal_comma() {
        t.format("%d/%m/%Y %H:%M:%S").to_string()
    } else {
        t.format("%Y-%m-%d %H:%M:%S").to_string()
    }
}

/// The kind column: "PNG image", "Folder"…
pub fn kind_label(ext: &str, is_dir: bool) -> String {
    let cat = Category::of(ext, is_dir);
    let name = crate::i18n::t(cat.key());
    if is_dir || ext.is_empty() {
        name.to_owned()
    } else {
        format!("{name} {}", ext.to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(format_size(999), "999 B");
        assert!(format_size(1_500_000).starts_with("1"));
        assert!(format_size(1_500_000).ends_with("MB"));
        assert!(format_size(25_000_000_000).ends_with("GB"));
    }

    #[test]
    fn categories() {
        assert_eq!(Category::of("jpg", false), Category::Image);
        assert_eq!(Category::of("tar.gz", false), Category::Archive);
        assert_eq!(Category::of("whatever", true), Category::Folder);
        assert_eq!(Category::of("qqq", false), Category::Other);
    }
}
