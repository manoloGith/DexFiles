//! Searching: by name (with `*` and `?`), content, kind, modification date and size, in a folder
//! or everything below it, on a worker thread that streams what it finds.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::fileinfo::Category;
use crate::vfs::{self, Entry, Kind, Location};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DateFilter {
    #[default]
    Any,
    Today,
    Week,
    Month,
    Year,
    OlderThanYear,
}

impl DateFilter {
    pub const ALL: [DateFilter; 6] = [
        DateFilter::Any,
        DateFilter::Today,
        DateFilter::Week,
        DateFilter::Month,
        DateFilter::Year,
        DateFilter::OlderThanYear,
    ];

    pub fn key(self) -> &'static str {
        match self {
            DateFilter::Any => "any_date",
            DateFilter::Today => "date_today",
            DateFilter::Week => "date_week",
            DateFilter::Month => "date_month",
            DateFilter::Year => "date_year",
            DateFilter::OlderThanYear => "date_older",
        }
    }

    fn matches(self, modified: Option<i64>, now: i64) -> bool {
        let day = 86_400;
        let Some(t) = modified else {
            return self == DateFilter::Any;
        };
        let age = now - t;
        match self {
            DateFilter::Any => true,
            DateFilter::Today => {
                use chrono::{Local, TimeZone};
                Local.timestamp_opt(t, 0).single().map(|d| d.date_naive())
                    == Some(Local::now().date_naive())
            }
            DateFilter::Week => age <= 7 * day,
            DateFilter::Month => age <= 31 * day,
            DateFilter::Year => age <= 366 * day,
            DateFilter::OlderThanYear => age > 366 * day,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SizeFilter {
    #[default]
    Any,
    Empty,
    Small,
    Medium,
    Large,
    Huge,
}

impl SizeFilter {
    pub const ALL: [SizeFilter; 6] = [
        SizeFilter::Any,
        SizeFilter::Empty,
        SizeFilter::Small,
        SizeFilter::Medium,
        SizeFilter::Large,
        SizeFilter::Huge,
    ];

    pub fn key(self) -> &'static str {
        match self {
            SizeFilter::Any => "any_size",
            SizeFilter::Empty => "size_empty",
            SizeFilter::Small => "size_small",
            SizeFilter::Medium => "size_medium",
            SizeFilter::Large => "size_large",
            SizeFilter::Huge => "size_huge",
        }
    }

    fn matches(self, e: &Entry) -> bool {
        const MB: u64 = 1_000_000;
        if self == SizeFilter::Any {
            return true;
        }
        if e.is_dir() {
            return false;
        }
        match self {
            SizeFilter::Any => true,
            SizeFilter::Empty => e.size == 0,
            SizeFilter::Small => e.size > 0 && e.size < MB,
            SizeFilter::Medium => (MB..100 * MB).contains(&e.size),
            SizeFilter::Large => (100 * MB..1000 * MB).contains(&e.size),
            SizeFilter::Huge => e.size >= 1000 * MB,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub text: String,
    /// Look inside files too (text, code and office documents).
    pub content: bool,
    pub category: Option<Category>,
    pub date: DateFilter,
    pub size: SizeFilter,
    /// Also below the folder.
    pub recursive: bool,
    pub include_hidden: bool,
}

impl Query {
    pub fn is_filtering(&self) -> bool {
        !self.text.trim().is_empty()
            || self.category.is_some()
            || self.date != DateFilter::Any
            || self.size != SizeFilter::Any
    }

    /// Whether an entry's name and metadata match (content is checked separately).
    pub fn matches_meta(&self, e: &Entry, now: i64) -> bool {
        if let Some(c) = self.category
            && Category::of(&e.ext, e.is_dir()) != c
        {
            return false;
        }
        if !self.date.matches(e.modified, now) || !self.size.matches(e) {
            return false;
        }
        let text = self.text.trim();
        text.is_empty() || name_matches(&e.name, text)
    }
}

fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Case- and accent-insensitive; `*` and `?` are wildcards (then the whole name must match),
/// otherwise every word must appear.
pub fn name_matches(name: &str, pattern: &str) -> bool {
    let name = fold(name);
    let pattern = fold(pattern);
    if pattern.contains('*') || pattern.contains('?') {
        return wildcard(
            &name.chars().collect::<Vec<_>>(),
            &pattern.chars().collect::<Vec<_>>(),
        );
    }
    pattern.split_whitespace().all(|w| name.contains(w))
}

fn wildcard(s: &[char], p: &[char]) -> bool {
    let (mut si, mut pi) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            si += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// A running (or finished) search.
pub struct Search {
    pub root: Location,
    pub query: Query,
    pub results: Arc<Mutex<Vec<Entry>>>,
    pub done: Arc<AtomicBool>,
    pub scanned: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
}

impl Search {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// Results found since the first `from` (to append, instead of copying them all each time).
    pub fn take_from(&self, from: usize) -> Vec<Entry> {
        let r = self.results.lock().unwrap_or_else(PoisonError::into_inner);
        r.get(from..).map(<[Entry]>::to_vec).unwrap_or_default()
    }

    pub fn count(&self) -> usize {
        self.results.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn snapshot(&self) -> Vec<Entry> {
        self.results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel();
    }
}

const CONTENT_LIMIT: usize = 16 * 1024 * 1024;

fn content_matches(e: &Entry, needle: &str) -> bool {
    if e.is_dir() || e.size as usize > CONTENT_LIMIT * 4 {
        return false;
    }
    let cat = Category::of(&e.ext, false);
    let text = match cat {
        Category::Document | Category::Spreadsheet | Category::Presentation => {
            match e.location.as_local() {
                Some(p) => crate::preview::office_text(p, 1_000_000).unwrap_or_default(),
                None => return false,
            }
        }
        _ => {
            let Ok(fs) = vfs::fs_for(&e.location) else {
                return false;
            };
            let Ok(bytes) = fs.read_head(&e.location.backend_path(), CONTENT_LIMIT) else {
                return false;
            };
            // Binary files (a NUL early on) are skipped unless their kind is text.
            if !cat.is_text() && bytes.iter().take(8000).any(|b| *b == 0) {
                return false;
            }
            String::from_utf8_lossy(&bytes).into_owned()
        }
    };
    let hay = fold(&text);
    fold(needle).split_whitespace().all(|w| hay.contains(w))
}

/// At most this many results (a search for "e" in a whole phone stays usable).
pub const MAX_RESULTS: usize = 20_000;

/// Folders a search never enters (virtual file systems).
const SKIP: &[&str] = &["/proc", "/sys", "/dev", "/run", "/storage/emulated/0/Android/data", "/storage/emulated/0/Android/obb"];

pub fn start(ctx: &egui::Context, root: Location, query: Query) -> Search {
    let search = Search {
        root: root.clone(),
        query: query.clone(),
        results: Arc::new(Mutex::new(Vec::new())),
        done: Arc::new(AtomicBool::new(false)),
        scanned: Arc::new(AtomicU64::new(0)),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let shared = Shared {
        results: search.results.clone(),
        scanned: search.scanned.clone(),
        cancel: search.cancel.clone(),
        ctx: ctx.clone(),
        last_repaint: Mutex::new(std::time::Instant::now()),
    };
    let done = search.done.clone();
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new().name("dexfiles-search".into()).spawn(move || {
        match &root {
            Location::Local(path) => search_local(path.clone(), &query, &shared),
            _ => search_listing(root.clone(), &query, &shared),
        }
        done.store(true, Ordering::SeqCst);
        ctx.request_repaint();
    });
    search
}

/// What the searching threads share.
struct Shared {
    results: Arc<Mutex<Vec<Entry>>>,
    scanned: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
    last_repaint: Mutex<std::time::Instant>,
}

impl Shared {
    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Hand over a batch of results; stop when there are enough.
    fn publish(&self, batch: &mut Vec<Entry>) {
        if batch.is_empty() {
            return;
        }
        let mut r = self.results.lock().unwrap_or_else(PoisonError::into_inner);
        r.append(batch);
        if r.len() >= MAX_RESULTS {
            r.truncate(MAX_RESULTS);
            self.cancel.store(true, Ordering::Relaxed);
        }
        drop(r);
        let mut last = self.last_repaint.lock().unwrap_or_else(PoisonError::into_inner);
        if last.elapsed().as_millis() > 120 {
            *last = std::time::Instant::now();
            self.ctx.request_repaint();
        }
    }
}

/// Whether an entry is a hit (its metadata is known).
fn is_hit(e: &Entry, query: &Query, meta_query: &Query, now: i64) -> bool {
    if query.content && !query.text.trim().is_empty() {
        meta_query.matches_meta(e, now) && (name_matches(&e.name, &query.text) || content_matches(e, &query.text))
    } else {
        query.matches_meta(e, now)
    }
}

/// Local folders: several threads take folders from a shared queue and read them with
/// `read_dir` alone (its file types need no extra system call); only candidates are looked at
/// further (their dates and sizes), and only when the query asks.
fn search_local(root: std::path::PathBuf, query: &Query, shared: &Shared) {
    use std::sync::Condvar;
    struct Queue {
        dirs: Vec<std::path::PathBuf>,
        busy: usize,
    }
    let queue = Mutex::new(Queue { dirs: vec![root], busy: 0 });
    let wake = Condvar::new();
    let now = chrono::Utc::now().timestamp();
    let mut meta_query = query.clone();
    if query.content {
        meta_query.text.clear();
    }
    let text = query.text.trim().to_owned();
    // Name-only queries can reject entries before looking at them closely.
    let name_gate = !text.is_empty() && !query.content;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(2, 6);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                let mut batch = Vec::new();
                loop {
                    let dir = {
                        let mut q = queue.lock().unwrap_or_else(PoisonError::into_inner);
                        loop {
                            if shared.stopped() {
                                q.dirs.clear();
                                wake.notify_all();
                                break None;
                            }
                            if let Some(d) = q.dirs.pop() {
                                q.busy += 1;
                                break Some(d);
                            }
                            if q.busy == 0 {
                                wake.notify_all();
                                break None;
                            }
                            q = wake.wait(q).unwrap_or_else(PoisonError::into_inner);
                        }
                    };
                    let Some(dir) = dir else { break };
                    let parent = Location::Local(dir.clone());
                    let mut subdirs = Vec::new();
                    if let Ok(rd) = std::fs::read_dir(&dir) {
                        for item in rd.flatten() {
                            if shared.stopped() {
                                break;
                            }
                            shared.scanned.fetch_add(1, Ordering::Relaxed);
                            let name = item.file_name().to_string_lossy().into_owned();
                            if name.starts_with('.') && !query.include_hidden {
                                continue;
                            }
                            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
                            if is_dir && query.recursive {
                                let p = item.path();
                                if !SKIP.iter().any(|s| p == std::path::Path::new(s)) {
                                    subdirs.push(p);
                                }
                            }
                            if name_gate && !name_matches(&name, &text) {
                                continue;
                            }
                            if let Some(c) = query.category
                                && Category::of(&if is_dir { String::new() } else { vfs::extension(&name) }, is_dir) != c
                            {
                                continue;
                            }
                            let Ok(raw) = vfs::local::raw_entry(&item.path(), name) else { continue };
                            let e = Entry::from_raw(&parent, raw);
                            if is_hit(&e, query, &meta_query, now) {
                                batch.push(e);
                                if batch.len() >= 64 {
                                    shared.publish(&mut batch);
                                }
                            }
                        }
                    }
                    shared.publish(&mut batch);
                    let mut q = queue.lock().unwrap_or_else(PoisonError::into_inner);
                    q.dirs.extend(subdirs);
                    q.busy -= 1;
                    wake.notify_all();
                }
            });
        }
    });
}

/// Servers: one folder at a time through the connection.
fn search_listing(root: Location, query: &Query, shared: &Shared) {
    let now = chrono::Utc::now().timestamp();
    let mut meta_query = query.clone();
    if query.content {
        meta_query.text.clear();
    }
    let mut stack = vec![root];
    let mut batch = Vec::new();
    while let Some(dir) = stack.pop() {
        if shared.stopped() {
            break;
        }
        let Ok(entries) = vfs::list(&dir) else { continue };
        for e in entries {
            shared.scanned.fetch_add(1, Ordering::Relaxed);
            if e.hidden && !query.include_hidden {
                continue;
            }
            if e.kind == Kind::Dir && query.recursive && !e.symlink {
                stack.push(e.location.clone());
            }
            if is_hit(&e, query, &meta_query, now) {
                batch.push(e);
            }
        }
        shared.publish(&mut batch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test --release search::tests::speed -- --ignored --nocapture`
    #[test]
    #[ignore = "a timing, not a check"]
    fn speed() {
        let ctx = egui::Context::default();
        let t0 = std::time::Instant::now();
        let q = Query { text: "lib".into(), recursive: true, ..Default::default() };
        let s = start(&ctx, Location::local("/usr"), q);
        while !s.is_done() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        println!("{} found, {} looked at, in {:?}", s.count(), s.scanned.load(Ordering::Relaxed), t0.elapsed());
    }

    #[test]
    fn names() {
        assert!(name_matches("Factura Enero.PDF", "factura"));
        assert!(name_matches("Canción.mp3", "cancion"));
        assert!(name_matches("IMG_2026_01.jpg", "img*.jpg"));
        assert!(!name_matches("IMG_2026_01.png", "img*.jpg"));
        assert!(name_matches("a1.txt", "a?.txt"));
        assert!(name_matches("informe final 2026.docx", "final informe"));
    }

    #[test]
    fn recursive_search_finds_content() {
        let root = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        std::fs::create_dir_all(root.path().join("x/y")).unwrap_or_default();
        std::fs::write(
            root.path().join("x/y/notes.txt"),
            "the secret word is pineapple",
        )
        .unwrap_or_default();
        std::fs::write(root.path().join("other.txt"), "nothing").unwrap_or_default();
        let ctx = egui::Context::default();
        let q = Query {
            text: "pineapple".into(),
            content: true,
            recursive: true,
            ..Default::default()
        };
        let s = start(&ctx, Location::local(root.path()), q);
        for _ in 0..300 {
            if s.is_done() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let r = s.snapshot();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "notes.txt");
    }
}
