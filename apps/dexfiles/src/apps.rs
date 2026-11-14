//! The apps that can open a kind of file, and opening a file with a chosen one: DexFiles' own
//! default apps (Settings ▸ Default apps), separate from the system's.
//!
//! On Android the activity lists the apps that handle the file's type (with their icons); on a
//! Linux desktop they come from the `.desktop` files that declare its MIME type.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppInfo {
    /// Android: `package/activity`; desktop: the `.desktop` file's id.
    pub id: String,
    pub label: String,
    pub icon: Option<egui::ColorImage>,
}

/// An app of the Dex family (DexPdfCraft…) and the extensions it says it opens. They become
/// DexFiles' default apps for those extensions (see `docs/apps-dex.md`).
#[derive(Clone, Debug)]
pub struct DexApp {
    /// As [`AppInfo::id`].
    pub id: String,
    pub label: String,
    /// Lowercase, without the dot.
    pub exts: Vec<String>,
}

/// What a Dex app declares: Android, an activity's `<meta-data>` of this name; desktop, a
/// `.desktop` key `X-DexFiles-Extensions`. The value lists extensions (`pdf, xps` or `pdf;xps`).
pub const EXTENSIONS_KEY: &str = "io.github.dexfiles.extensions";

/// The extensions of a declaration: lowercase, without dots, no repeats.
pub fn parse_extensions(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in value.split([',', ';', ' ', '\t']) {
        let e = e
            .trim()
            .trim_start_matches("*.")
            .trim_start_matches('.')
            .to_lowercase();
        if !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()) && !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

/// The installed Dex apps that declare their extensions (blocks: run on a worker).
/// `None` when the system couldn't be asked (then nothing is concluded from it).
pub fn dex_apps() -> Option<Vec<DexApp>> {
    #[cfg(target_os = "android")]
    {
        crate::android::dex_apps()
    }
    #[cfg(not(target_os = "android"))]
    {
        Some(
            desktop::entries()
                .into_iter()
                .filter(|e| !e.dex_extensions.is_empty())
                .map(|e| DexApp {
                    id: e.id,
                    label: e.name,
                    exts: e.dex_extensions,
                })
                .collect(),
        )
    }
}

/// Which of these apps (as [`AppInfo::id`]) are installed; `None` when the system couldn't be
/// asked. Blocks: run on a worker.
pub fn installed(ids: &[String]) -> Option<Vec<String>> {
    #[cfg(target_os = "android")]
    {
        crate::android::installed(ids)
    }
    #[cfg(not(target_os = "android"))]
    {
        let entries = desktop::entries();
        if entries.is_empty() {
            return None;
        }
        Some(
            ids.iter()
                .filter(|id| entries.iter().any(|e| e.id == **id))
                .cloned()
                .collect(),
        )
    }
}

/// The MIME type of an extension (`application/octet-stream` if unknown).
pub fn mime_for(ext: &str) -> String {
    match ext {
        "7z" => "application/x-7z-compressed".to_owned(),
        "rar" => "application/vnd.rar".to_owned(),
        "md" | "log" | "rs" | "toml" | "kt" | "kts" | "conf" | "ini" => "text/plain".to_owned(),
        _ => mime_guess::from_ext(ext)
            .first()
            .map(|m| m.essence_str().to_owned())
            .unwrap_or_else(|| "application/octet-stream".to_owned()),
    }
}

/// The apps that can open files with this extension (blocks: run on a worker).
pub fn apps_for(ext: &str) -> Vec<AppInfo> {
    #[cfg(target_os = "android")]
    {
        crate::android::apps_for(ext, &mime_for(ext))
    }
    #[cfg(not(target_os = "android"))]
    {
        desktop::apps_for(&mime_for(ext))
    }
}

/// Open a local file with one app.
pub fn open_with(path: &Path, app: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::open_with(path, app)
    }
    #[cfg(not(target_os = "android"))]
    {
        desktop::open_with(path, app)
    }
}

/// Decode an icon handed over as PNG.
pub fn icon_from_png(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    ))
}

#[cfg_attr(target_os = "android", allow(dead_code))]
pub mod desktop {
    //! `.desktop` files (freedesktop.org Desktop Entry Specification).

    use super::{AppInfo, Path, PathBuf};

    pub struct DesktopEntry {
        pub id: String,
        pub name: String,
        pub exec: String,
        pub mime_types: Vec<String>,
        /// `X-DexFiles-Extensions`: a Dex app's own extensions.
        pub dex_extensions: Vec<String>,
    }

    fn dirs() -> Vec<PathBuf> {
        let mut out = Vec::new();
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        if let Some(d) = data_home {
            out.push(d.join("applications"));
        }
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
        for d in data_dirs.split(':').filter(|d| !d.is_empty()) {
            out.push(PathBuf::from(d).join("applications"));
        }
        out
    }

    /// Parse the `[Desktop Entry]` group of a `.desktop` file (the localized name if there is one).
    pub fn parse(id: &str, text: &str, lang: &str) -> Option<DesktopEntry> {
        let mut in_entry = false;
        let (mut name, mut local_name, mut exec, mut mime) = (None, None, None, Vec::new());
        let mut dex = Vec::new();
        let mut hidden = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "Name" => name = Some(v.to_owned()),
                "Exec" => exec = Some(v.to_owned()),
                "MimeType" => {
                    mime = v
                        .split(';')
                        .filter(|m| !m.is_empty())
                        .map(str::to_owned)
                        .collect()
                }
                "X-DexFiles-Extensions" => dex = super::parse_extensions(v),
                "NoDisplay" | "Hidden" if v == "true" => hidden = true,
                "Type" if v != "Application" => hidden = true,
                _ if !lang.is_empty() && k == format!("Name[{lang}]") => {
                    local_name = Some(v.to_owned())
                }
                _ => {}
            }
        }
        if hidden {
            return None;
        }
        Some(DesktopEntry {
            id: id.to_owned(),
            name: local_name.or(name)?,
            exec: exec?,
            mime_types: mime,
            dex_extensions: dex,
        })
    }

    pub fn entries() -> Vec<DesktopEntry> {
        let lang = std::env::var("LANG")
            .unwrap_or_default()
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect::<String>();
        let mut out: Vec<DesktopEntry> = Vec::new();
        for dir in dirs() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for item in rd.flatten() {
                let p = item.path();
                if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let id = item.file_name().to_string_lossy().into_owned();
                if out.iter().any(|e| e.id == id) {
                    continue; // the user's own file wins
                }
                if let Some(e) = std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|t| parse(&id, &t, &lang))
                {
                    out.push(e);
                }
            }
        }
        out
    }

    pub fn apps_for(mime: &str) -> Vec<AppInfo> {
        let family = format!("{}/*", mime.split('/').next().unwrap_or_default());
        let mut apps: Vec<AppInfo> = entries()
            .into_iter()
            .filter(|e| e.mime_types.iter().any(|m| m == mime || *m == family))
            .map(|e| AppInfo {
                id: e.id,
                label: e.name,
                icon: None,
            })
            .collect();
        apps.sort_by_key(|a| a.label.to_lowercase());
        apps
    }

    /// The command line of an `Exec` key for one file (field codes replaced, quoting honoured).
    pub fn command(exec: &str, file: &Path) -> Vec<String> {
        let mut words = Vec::new();
        let mut cur = String::new();
        let mut quoted = false;
        let mut chars = exec.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => quoted = !quoted,
                '\\' if quoted => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                }
                ' ' if !quoted => {
                    if !cur.is_empty() {
                        words.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(c),
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }
        let path = file.to_string_lossy().into_owned();
        let mut out = Vec::new();
        let mut used = false;
        for w in words {
            match w.as_str() {
                "%f" | "%F" | "%u" | "%U" => {
                    out.push(path.clone());
                    used = true;
                }
                "%i" | "%c" | "%k" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
                _ => out.push(w.replace("%%", "%")),
            }
        }
        if !used {
            out.push(path);
        }
        out
    }

    pub fn open_with(path: &Path, id: &str) -> Result<(), String> {
        let entry = entries()
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("{id}: not installed"))?;
        let cmd = command(&entry.exec, path);
        let (prog, args) = cmd.split_first().ok_or_else(|| "empty Exec".to_owned())?;
        std::process::Command::new(prog)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("{prog}: {e}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn desktop_files() {
            let text = "[Desktop Entry]\nType=Application\nName=Image Viewer\nName[es]=Visor de imágenes\nExec=eog %U\nMimeType=image/png;image/jpeg;\n\n[Desktop Action new]\nName=Other\n";
            let e = parse("eog.desktop", text, "es").unwrap_or_else(|| unreachable!());
            assert_eq!(e.name, "Visor de imágenes");
            assert_eq!(e.mime_types, vec!["image/png", "image/jpeg"]);
            assert!(
                parse(
                    "x.desktop",
                    "[Desktop Entry]\nName=X\nExec=x\nNoDisplay=true\n",
                    ""
                )
                .is_none()
            );
        }

        #[test]
        fn exec_lines() {
            let p = Path::new("/tmp/a b.png");
            assert_eq!(command("eog %U", p), vec!["eog", "/tmp/a b.png"]);
            assert_eq!(
                command("\"/opt/my app/run\" --open %f --icon %i", p),
                vec!["/opt/my app/run", "--open", "/tmp/a b.png", "--icon"]
            );
            assert_eq!(command("gimp", p), vec!["gimp", "/tmp/a b.png"]);
        }
    }
}

#[cfg(test)]
mod dex_tests {
    #[test]
    fn declarations() {
        assert_eq!(
            super::parse_extensions(" PDF, .xps;*.oxps  pdf"),
            vec!["pdf", "xps", "oxps"]
        );
        let e = super::desktop::parse(
            "dexpdfcraft.desktop",
            "[Desktop Entry]\nType=Application\nName=DexPdfCraft\nExec=dexpdfcraft %f\nX-DexFiles-Extensions=pdf;\n",
            "",
        )
        .unwrap_or_else(|| unreachable!());
        assert_eq!(e.dex_extensions, vec!["pdf"]);
    }
}
