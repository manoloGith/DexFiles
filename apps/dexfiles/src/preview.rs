//! What the preview panel shows for a file, and the thumbnails of the icon views. Loaded on
//! worker threads; images arrive as `egui::ColorImage` for the interface to upload.
//!
//! Phase 1: images, text and code, office documents as text (DOCX, ODT, PPTX, ODP), spreadsheets
//! as a table (XLSX, XLS, ODS, CSV), archives as their file list, folders as their item count.
//! PDF pages come in phase 2 (see docs/dexfiles/plan.md).

use std::io::{Cursor, Read};
use std::path::Path;

use crate::fileinfo::Category;
use crate::vfs::{self, Entry};

pub enum Content {
    Image {
        image: egui::ColorImage,
        width: u32,
        height: u32,
    },
    /// Audio or video: its picture (cover art, a video frame) and what is known about it.
    Media {
        image: Option<egui::ColorImage>,
        /// (i18n key, value)
        info: Vec<(&'static str, String)>,
    },
    /// A PDF's first page, and how many pages it has.
    Pdf {
        image: egui::ColorImage,
        pages: usize,
        /// Which page (from 0), and the local file, to render another page or size.
        page: usize,
        path: std::path::PathBuf,
        /// The longer side it was rendered at, in pixels.
        px: u32,
    },
    Text {
        text: String,
        truncated: bool,
    },
    Document {
        text: String,
        truncated: bool,
    },
    Table {
        sheets: Vec<String>,
        rows: Vec<Vec<String>>,
        truncated: bool,
    },
    Archive {
        items: Vec<crate::archive::ArchiveItem>,
        count: usize,
    },
    Folder {
        items: usize,
        folders: usize,
    },
    /// Nothing to show inside (the i18n key of why).
    None(&'static str),
    Error(String),
}

const IMAGE_LIMIT: usize = 80 * 1024 * 1024;
const TEXT_LIMIT: usize = 256 * 1024;

/// Decode an image (honouring EXIF rotation) and shrink it to fit `max` pixels.
pub fn decode_image(bytes: &[u8], max: u32) -> Result<(egui::ColorImage, u32, u32), String> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().ok();
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let (w, h) = (img.width(), img.height());
    let img = if w > max || h > max {
        img.thumbnail(max, max)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok((
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        w,
        h,
    ))
}

fn decodable_image(ext: &str) -> bool {
    matches!(
        ext,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff"
    )
}

/// A thumbnail (`size` pixels at most), for images.
/// PDFs bigger than this are not previewed (they are read whole).
const PDF_LIMIT: usize = 120 * 1024 * 1024;

/// A PDF's first page as an image fitting `max` pixels, and its page count (hayro, the pure
/// Rust renderer PdfCraft uses).
pub fn render_pdf(
    bytes: Vec<u8>,
    page_index: usize,
    max: u32,
) -> Result<(egui::ColorImage, usize), String> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    let pdf = Pdf::new(bytes).map_err(|e| format!("{e:?}"))?;
    let pages = pdf.pages();
    let count = pages.len();
    let page = pages
        .get(page_index.min(count.saturating_sub(1)))
        .ok_or_else(|| "no pages".to_owned())?;
    let (w, h) = page.render_dimensions();
    // Rendered at the size it is shown, in device pixels (sharp text), within reason.
    let scale = (max.clamp(64, 4096) as f32 / w.max(h).max(1.0)).min(8.0);
    let cache = hayro::RenderCache::new();
    let pixmap_settings = hayro::PixmapSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
    };
    let pixmap = hayro::render(
        page,
        &cache,
        &InterpreterSettings::default(),
        &hayro::RenderSettings::default(),
        &pixmap_settings,
    );
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    Ok((
        egui::ColorImage::from_rgba_premultiplied(size, pixmap.data_as_u8_slice()),
        count,
    ))
}

/// One page of a local PDF, `px` pixels on its longer side.
pub fn pdf_page(path: &Path, page: usize, px: u32) -> Content {
    match std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|b| render_pdf(b, page, px))
    {
        Ok((image, pages)) => Content::Pdf {
            image,
            pages,
            page: page.min(pages.saturating_sub(1)),
            path: path.to_path_buf(),
            px,
        },
        Err(e) => Content::Error(e),
    }
}

/// A picture (cover art, a frame) and (i18n key, value) details of an audio or video file.
pub type MediaInfo = (Option<egui::ColorImage>, Vec<(&'static str, String)>);

/// An audio file's tags and cover art (lofty: MP3, FLAC, Ogg, Opus, M4A, WAV, AIFF…).
pub fn audio(path: &Path, max: u32) -> Option<MediaInfo> {
    use lofty::prelude::*;
    let file = lofty::read_from_path(path).ok()?;
    let props = file.properties();
    let mut info = Vec::new();
    let tag = file.primary_tag().or_else(|| file.first_tag());
    if let Some(tag) = tag {
        for (key, value) in [
            ("media_title", tag.title()),
            ("media_artist", tag.artist()),
            ("media_album", tag.album()),
            ("media_genre", tag.genre()),
        ] {
            if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
                info.push((key, v.into_owned()));
            }
        }
        if let Some(y) = tag.date() {
            info.push(("media_year", y.year.to_string()));
        }
    }
    info.push((
        "media_duration",
        format_duration(props.duration().as_secs()),
    ));
    if let Some(b) = props.audio_bitrate() {
        info.push(("media_bitrate", format!("{b} kbps")));
    }
    if let Some(r) = props.sample_rate() {
        let khz = format!("{:.1} kHz", f64::from(r) / 1000.0);
        info.push((
            "media_sample_rate",
            if crate::i18n::decimal_comma() {
                khz.replace('.', ",")
            } else {
                khz
            },
        ));
    }
    let cover = tag
        .and_then(|t| t.pictures().first())
        .and_then(|p| decode_image(p.data(), max).ok())
        .map(|(i, _, _)| i);
    Some((cover, info))
}

pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A video's frame and details: the system decodes it on Android; elsewhere nothing.
pub fn video(path: &Path, max: u32) -> Option<MediaInfo> {
    #[cfg(target_os = "android")]
    {
        crate::android::video_frame(path, max)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, max);
        None
    }
}

pub fn thumbnail(entry: &Entry, size: u32) -> Option<egui::ColorImage> {
    let cat = Category::of(&entry.ext, false);
    if matches!(cat, Category::Audio | Category::Video) {
        let path = entry.location.as_local()?;
        let found = if cat == Category::Audio {
            audio(path, size)
        } else {
            video(path, size)
        };
        return found.and_then(|(img, _)| img);
    }
    if entry.ext == "pdf" && entry.size as usize <= PDF_LIMIT {
        let bytes = read(entry, PDF_LIMIT).ok()?;
        return render_pdf(bytes, 0, size).ok().map(|(i, _)| i);
    }
    if !decodable_image(&entry.ext) || entry.size as usize > IMAGE_LIMIT {
        return None;
    }
    let bytes = read(entry, IMAGE_LIMIT).ok()?;
    decode_image(&bytes, size).ok().map(|(i, _, _)| i)
}

fn read(entry: &Entry, limit: usize) -> std::io::Result<Vec<u8>> {
    let fs = vfs::fs_for(&entry.location)?;
    fs.read_head(&entry.location.backend_path(), limit)
}

/// Text of the XML parts of a zip-based office document, paragraphs on their own lines.
fn xml_text(
    xml: &str,
    paragraph_tags: &[&str],
    text_tags: &[&str],
    out: &mut String,
    limit: usize,
) {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut in_text = 0usize;
    loop {
        if out.len() >= limit {
            return;
        }
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let name: &str = name.as_ref();
                if text_tags.contains(&name) {
                    in_text += 1;
                }
                if name == "text:tab" || name == "w:tab" {
                    out.push('\t');
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let name: &str = name.as_ref();
                match name {
                    "w:tab" | "text:tab" => out.push('\t'),
                    "w:br" | "text:line-break" | "a:br" => out.push('\n'),
                    "text:s" => out.push(' '),
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if in_text > 0 => {
                out.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(r)) if in_text > 0 => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    out.push(c);
                } else {
                    match r.as_ref() {
                        "amp" => out.push('&'),
                        "lt" => out.push('<'),
                        "gt" => out.push('>'),
                        "quot" => out.push('"'),
                        "apos" => out.push('\''),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let name: &str = name.as_ref();
                if text_tags.contains(&name) {
                    in_text = in_text.saturating_sub(1);
                }
                if paragraph_tags.contains(&name) {
                    out.push('\n');
                }
            }
            Ok(Event::Eof) | Err(_) => return,
            _ => {}
        }
    }
}

/// The text of an office document (DOCX, ODT, PPTX, ODP, or a spreadsheet's cells).
pub fn office_text(path: &Path, limit: usize) -> Option<String> {
    let ext = vfs::extension(&path.file_name()?.to_string_lossy());
    if Category::of(&ext, false) == Category::Spreadsheet {
        let (_, rows, _) = table(path, 2000, 50).ok()?;
        return Some(
            rows.into_iter()
                .map(|r| r.join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let mut out = String::new();
    let part = |zip: &mut zip::ZipArchive<_>,
                name: &str,
                para: &[&str],
                text: &[&str],
                out: &mut String| {
        if let Ok(mut f) = zip.by_name(name) {
            let mut xml = String::new();
            if f.read_to_string(&mut xml).is_ok() {
                xml_text(&xml, para, text, out, limit);
            }
        }
    };
    match ext.as_str() {
        "docx" => part(&mut zip, "word/document.xml", &["w:p"], &["w:t"], &mut out),
        "odt" | "odp" => part(
            &mut zip,
            "content.xml",
            &["text:p", "text:h"],
            &["text:p", "text:h", "text:span"],
            &mut out,
        ),
        "pptx" => {
            let mut slides: Vec<String> = zip
                .file_names()
                .filter_map(|n| n.ok().map(|n| n.into_owned()))
                .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
                .collect();
            slides.sort_by_key(|n| {
                n.trim_start_matches("ppt/slides/slide")
                    .trim_end_matches(".xml")
                    .parse::<u32>()
                    .unwrap_or(0)
            });
            for (i, s) in slides.iter().enumerate() {
                out.push_str(&format!("— {} {} —\n", crate::i18n::t("slide"), i + 1));
                part(&mut zip, s, &["a:p"], &["a:t"], &mut out);
                out.push('\n');
            }
        }
        _ => return None,
    }
    Some(out)
}

/// A spreadsheet's first sheet as text cells: (sheet names, rows, truncated).
/// Sheet names, rows of cells, truncated.
type Table = (Vec<String>, Vec<Vec<String>>, bool);

fn table(path: &Path, max_rows: usize, max_cols: usize) -> Result<Table, String> {
    let ext = vfs::extension(
        &path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    if ext == "csv" || ext == "tsv" {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(TEXT_LIMIT as u64 * 4).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let sep = if ext == "tsv" {
            '\t'
        } else {
            let first = text.lines().next().unwrap_or_default();
            if first.matches(';').count() > first.matches(',').count() {
                ';'
            } else {
                ','
            }
        };
        let mut rows = Vec::new();
        for line in text.lines() {
            if rows.len() >= max_rows {
                return Ok((Vec::new(), rows, true));
            }
            rows.push(split_csv(line, sep).into_iter().take(max_cols).collect());
        }
        return Ok((Vec::new(), rows, false));
    }
    use calamine::Reader;
    let mut book = calamine::open_workbook_auto(path).map_err(|e| e.to_string())?;
    let sheets = book.sheet_names();
    let first = sheets
        .first()
        .cloned()
        .ok_or_else(|| "no sheets".to_owned())?;
    let range = book.worksheet_range(&first).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut truncated = false;
    for row in range.rows() {
        if rows.len() >= max_rows {
            truncated = true;
            break;
        }
        rows.push(row.iter().take(max_cols).map(|c| c.to_string()).collect());
    }
    Ok((sheets, rows, truncated))
}

fn split_csv(line: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The preview of an entry (blocks: run on a worker).
pub fn load(entry: &Entry, max_px: u32) -> Content {
    if entry.is_dir() {
        return match vfs::list(&entry.location) {
            Ok(items) => Content::Folder {
                folders: items.iter().filter(|e| e.is_dir()).count(),
                items: items.len(),
            },
            Err(e) => Content::Error(e.to_string()),
        };
    }
    let cat = Category::of(&entry.ext, false);
    if decodable_image(&entry.ext) {
        if entry.size as usize > IMAGE_LIMIT {
            return Content::None("preview_too_big");
        }
        return match read(entry, IMAGE_LIMIT)
            .map_err(|e| e.to_string())
            .and_then(|b| decode_image(&b, max_px))
        {
            Ok((image, width, height)) => Content::Image {
                image,
                width,
                height,
            },
            Err(e) => Content::Error(e),
        };
    }
    match cat {
        Category::Text | Category::Code => match read(entry, TEXT_LIMIT) {
            Ok(bytes) => Content::Text {
                truncated: bytes.len() >= TEXT_LIMIT,
                text: String::from_utf8_lossy(&bytes).into_owned(),
            },
            Err(e) => Content::Error(e.to_string()),
        },
        Category::Document | Category::Presentation | Category::Spreadsheet | Category::Archive => {
            // These need the whole file locally.
            let path = match vfs::local_copy(&entry.location, &mut |n| n < 200 * 1024 * 1024) {
                Ok(p) => p,
                Err(e) => return Content::Error(e.to_string()),
            };
            match cat {
                Category::Spreadsheet => match table(&path, 200, 30) {
                    Ok((sheets, rows, truncated)) => Content::Table {
                        sheets,
                        rows,
                        truncated,
                    },
                    Err(e) => Content::Error(e),
                },
                Category::Archive => match crate::archive::list(&path, 500) {
                    Ok((items, count)) => Content::Archive { items, count },
                    Err(_) => Content::None("preview_unsupported"),
                },
                _ => match office_text(&path, 100_000) {
                    Some(text) if !text.trim().is_empty() => Content::Document {
                        truncated: text.len() >= 100_000,
                        text,
                    },
                    Some(_) => Content::None("preview_empty"),
                    None => Content::None("preview_unsupported"),
                },
            }
        }
        Category::Audio | Category::Video => {
            let path = match vfs::local_copy(&entry.location, &mut |n| n < 300 * 1024 * 1024) {
                Ok(p) => p,
                Err(e) => return Content::Error(e.to_string()),
            };
            let found = if cat == Category::Audio {
                audio(&path, max_px.min(800))
            } else {
                video(&path, max_px.min(1200))
            };
            match found {
                Some((image, info)) => Content::Media { image, info },
                None if cat == Category::Video => Content::None("preview_video_device"),
                None => Content::None("preview_unsupported"),
            }
        }
        Category::Pdf => {
            if entry.size as usize > PDF_LIMIT {
                return Content::None("preview_too_big");
            }
            let path = match vfs::local_copy(&entry.location, &mut |_| true) {
                Ok(p) => p,
                Err(e) => return Content::Error(e.to_string()),
            };
            pdf_page(&path, 0, max_px)
        }
        _ => {
            // Unknown types: show them as text if they look like text.
            if entry.size > 0
                && entry.size < 4 * 1024 * 1024
                && let Ok(bytes) = read(entry, TEXT_LIMIT)
                && !bytes.iter().take(4096).any(|b| *b == 0)
                && std::str::from_utf8(&bytes[..bytes.len().min(4096)]).is_ok()
            {
                return Content::Text {
                    truncated: bytes.len() >= TEXT_LIMIT,
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                };
            }
            Content::None("preview_unsupported")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn audio_details() {
        // two seconds of silence, 8 kHz mono 16-bit
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("tone.wav");
        let samples = 16_000u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        wav.resize(wav.len() + samples as usize * 2, 0);
        std::fs::write(&path, wav).unwrap_or_default();
        let (_, info) = audio(&path, 100).unwrap_or_else(|| unreachable!());
        assert!(
            info.contains(&("media_duration", "0:02".to_owned())),
            "{info:?}"
        );
        assert!(
            info.iter()
                .any(|(k, v)| *k == "media_sample_rate" && v.starts_with("8")),
            "{info:?}"
        );
    }

    #[test]
    fn csv_cells() {
        assert_eq!(split_csv(r#"a,"b,c",d"#, ','), vec!["a", "b,c", "d"]);
        assert_eq!(
            split_csv(r#""say ""hi""";2"#, ';'),
            vec![r#"say "hi""#, "2"]
        );
    }

    #[test]
    fn docx_text() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("t.docx");
        let mut zip =
            zip::ZipWriter::new(std::fs::File::create(&path).unwrap_or_else(|_| unreachable!()));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap_or_default();
        zip.write_all(br#"<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>Hola</w:t></w:r><w:r><w:t xml:space="preserve"> mundo &amp; co</w:t></w:r></w:p><w:p><w:r><w:t>Dos</w:t></w:r></w:p></w:body></w:document>"#).unwrap_or_default();
        zip.finish().unwrap_or_else(|_| unreachable!());
        assert_eq!(
            office_text(&path, 1000).unwrap_or_default(),
            "Hola mundo & co\nDos\n"
        );
    }

    #[test]
    fn images_shrink() {
        let img = image::RgbaImage::from_pixel(400, 200, image::Rgba([10, 20, 30, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap_or_default();
        let (i, w, h) = decode_image(&png, 100).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!((w, h), (400, 200));
        assert_eq!(i.size, [100, 50]);
    }
}
