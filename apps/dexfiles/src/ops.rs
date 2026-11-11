//! File operations that run in the background: copy and move (between any two places, local or
//! remote), delete, trash, restore, compress, extract, new folders, renames. Each is a [`Job`]
//! the interface lists with its progress; a job may stop to ask what to do with a name that is
//! already taken ([`Question`]).

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Instant;
#[cfg(test)]
use std::time::Duration;

use crate::archive::ArchiveFormat;
use crate::trash::TrashInfo;
use crate::vfs::{self, Fs, Kind, Location};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Copy,
    Move,
}

/// What to do when the target name is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    Replace,
    Skip,
    KeepBoth,
}

#[derive(Clone, Debug)]
pub enum JobKind {
    Transfer { sources: Vec<Location>, dest: Location, mode: Mode },
    Delete { items: Vec<Location> },
    Trash { items: Vec<PathBuf> },
    Restore { items: Vec<TrashInfo> },
    Purge { items: Vec<TrashInfo> },
    EmptyTrash,
    Compress { sources: Vec<Location>, dest: Location, format: ArchiveFormat },
    Extract { archive: Location, dest: Location },
    Mkdir { location: Location },
    NewFile { location: Location },
    Rename { from: Location, to: Location },
}

#[derive(Clone, Debug)]
pub struct Question {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Answer {
    pub conflict: Conflict,
    pub apply_to_all: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub done_items: u64,
    pub total_items: u64,
    pub current: String,
    pub finished: bool,
    pub cancelled: bool,
    pub errors: Vec<String>,
    /// Folders whose listings changed.
    pub touched: Vec<Location>,
    /// What the job made (a new folder, a renamed file), to select it.
    pub created: Vec<Location>,
    pub question: Option<Question>,
}

pub struct JobState {
    cancel: AtomicBool,
    status: Mutex<Status>,
    answer: Mutex<Option<Answer>>,
    answered: Condvar,
    ctx: egui::Context,
    last_repaint: AtomicU64,
}

pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    pub state: Arc<JobState>,
    pub started: Instant,
    /// Shown in the list of jobs (quick jobs such as a rename are not, unless they fail).
    pub visible: bool,
}

impl JobState {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        // A job waiting for an answer stops too.
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(Answer { conflict: Conflict::Skip, apply_to_all: true });
        self.answered.notify_all();
    }

    pub fn answer(&self, answer: Answer) {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).question = None;
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
        self.answered.notify_all();
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn update(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().unwrap_or_else(PoisonError::into_inner));
        // Repaint at most every 100 ms while bytes flow.
        let now = epoch_ms();
        let last = self.last_repaint.load(Ordering::Relaxed);
        if now.saturating_sub(last) > 100 {
            self.last_repaint.store(now, Ordering::Relaxed);
            self.ctx.request_repaint();
        }
    }

    fn error(&self, what: &str, e: &io::Error) {
        let msg = format!("{what}: {e}");
        log::warn!("dexfiles: {msg}");
        self.update(|s| s.errors.push(msg));
    }

    fn touch(&self, dir: Option<Location>) {
        if let Some(d) = dir {
            self.update(|s| {
                if !s.touched.contains(&d) {
                    s.touched.push(d);
                }
            });
        }
    }

    /// Ask the interface; blocks until it answers.
    fn ask(&self, question: Question) -> Answer {
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.update(|s| s.question = Some(question));
        self.ctx.request_repaint();
        let mut answer = self.answer.lock().unwrap_or_else(PoisonError::into_inner);
        while answer.is_none() {
            answer = self.answered.wait(answer).unwrap_or_else(PoisonError::into_inner);
        }
        answer.unwrap_or(Answer { conflict: Conflict::Skip, apply_to_all: false })
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| u64::try_from(d.as_millis()).unwrap_or(0)).unwrap_or(0)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Start a job on its own thread.
pub fn start(ctx: &egui::Context, kind: JobKind) -> Job {
    let visible = !matches!(kind, JobKind::Mkdir { .. } | JobKind::NewFile { .. } | JobKind::Rename { .. });
    let state = Arc::new(JobState {
        cancel: AtomicBool::new(false),
        status: Mutex::new(Status::default()),
        answer: Mutex::new(None),
        answered: Condvar::new(),
        ctx: ctx.clone(),
        last_repaint: AtomicU64::new(0),
    });
    let job = Job { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), kind: kind.clone(), state: state.clone(), started: Instant::now(), visible };
    let spawned = std::thread::Builder::new().name("dexfiles-job".into()).spawn(move || {
        run(&state, kind);
        let cancelled = state.cancelled();
        state.update(|s| {
            s.finished = true;
            s.cancelled = cancelled;
            s.current.clear();
        });
        state.ctx.request_repaint();
    });
    if let Err(e) = spawned {
        job.state.update(|s| {
            s.errors.push(e.to_string());
            s.finished = true;
        });
    }
    job
}

fn run(state: &JobState, kind: JobKind) {
    match kind {
        JobKind::Transfer { sources, dest, mode } => transfer(state, &sources, &dest, mode),
        JobKind::Delete { items } => {
            state.update(|s| s.total_items = items.len() as u64);
            for item in items {
                if state.cancelled() {
                    break;
                }
                state.update(|s| s.current = item.name());
                let r = vfs::fs_for(&item).and_then(|fs| delete_tree(state, fs.as_ref(), &item));
                if let Err(e) = r {
                    if !vfs::is_cancelled(&e) {
                        state.error(&item.name(), &e);
                    }
                }
                state.touch(item.parent());
                state.update(|s| s.done_items += 1);
            }
        }
        JobKind::Trash { items } => {
            state.update(|s| s.total_items = items.len() as u64);
            for p in items {
                if state.cancelled() {
                    break;
                }
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                state.update(|s| s.current = name.clone());
                if let Err(e) = crate::trash::trash(&p) {
                    state.error(&name, &e);
                }
                state.touch(p.parent().map(|d| Location::Local(d.to_path_buf())));
                state.update(|s| s.done_items += 1);
            }
            state.touch(Some(Location::Trash));
        }
        JobKind::Restore { items } => {
            state.update(|s| s.total_items = items.len() as u64);
            for t in items {
                match crate::trash::restore(&t) {
                    Ok(p) => {
                        state.touch(p.parent().map(|d| Location::Local(d.to_path_buf())));
                        state.update(|s| s.created.push(Location::Local(p)));
                    }
                    Err(e) => state.error(&t.original.to_string_lossy(), &e),
                }
                state.update(|s| s.done_items += 1);
            }
            state.touch(Some(Location::Trash));
        }
        JobKind::Purge { items } => {
            state.update(|s| s.total_items = items.len() as u64);
            for t in items {
                if let Err(e) = crate::trash::purge(&t) {
                    state.error(&t.original.to_string_lossy(), &e);
                }
                state.update(|s| s.done_items += 1);
            }
            state.touch(Some(Location::Trash));
        }
        JobKind::EmptyTrash => {
            if let Err(e) = crate::trash::empty() {
                state.error("trash", &e);
            }
            state.touch(Some(Location::Trash));
        }
        JobKind::Compress { sources, dest, format } => {
            let (Some(target), true) = (dest.as_local(), sources.iter().all(Location::is_local)) else {
                state.error("compress", &vfs::other("only local files can be compressed"));
                return;
            };
            let paths: Vec<PathBuf> = sources.iter().filter_map(|l| l.as_local().map(PathBuf::from)).collect();
            let r = crate::archive::compress(&paths, target, format, &mut |done, total, name| {
                state.update(|s| {
                    s.done_bytes = done;
                    s.total_bytes = total;
                    s.current = name.to_owned();
                });
                !state.cancelled()
            });
            match r {
                Ok(()) => state.update(|s| s.created.push(dest.clone())),
                Err(e) => {
                    let _ = std::fs::remove_file(target);
                    if !vfs::is_cancelled(&e) {
                        state.error(&dest.name(), &e);
                    }
                }
            }
            state.touch(dest.parent());
        }
        JobKind::Extract { archive, dest } => {
            let local = match vfs::local_copy(&archive, &mut |_| !state.cancelled()) {
                Ok(p) => p,
                Err(e) => return state.error(&archive.name(), &e),
            };
            let Some(target) = dest.as_local() else {
                state.error("extract", &vfs::other("archives are extracted to local folders"));
                return;
            };
            let r = crate::archive::extract(&local, target, &mut |done, total, name| {
                state.update(|s| {
                    s.done_bytes = done;
                    s.total_bytes = total;
                    s.current = name.to_owned();
                });
                !state.cancelled()
            });
            if let Err(e) = r {
                if !vfs::is_cancelled(&e) {
                    state.error(&archive.name(), &e);
                }
            } else {
                state.update(|s| s.created.push(dest.clone()));
            }
            state.touch(dest.parent());
            state.touch(Some(dest));
        }
        JobKind::Mkdir { location } => {
            match vfs::fs_for(&location).and_then(|fs| fs.mkdir(&location.backend_path())) {
                Ok(()) => state.update(|s| s.created.push(location.clone())),
                Err(e) => state.error(&location.name(), &e),
            }
            state.touch(location.parent());
        }
        JobKind::NewFile { location } => {
            let r = vfs::fs_for(&location).and_then(|fs| fs.write_from(&location.backend_path(), &mut io::empty(), 0, &mut |_| true));
            match r {
                Ok(_) => state.update(|s| s.created.push(location.clone())),
                Err(e) => state.error(&location.name(), &e),
            }
            state.touch(location.parent());
        }
        JobKind::Rename { from, to } => {
            let r = vfs::fs_for(&from).and_then(|fs| {
                if exists(fs.as_ref(), &to) && from.name().to_lowercase() != to.name().to_lowercase() {
                    return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the name is taken"));
                }
                fs.rename(&from.backend_path(), &to.backend_path())
            });
            match r {
                Ok(()) => state.update(|s| s.created.push(to.clone())),
                Err(e) => state.error(&from.name(), &e),
            }
            state.touch(from.parent());
        }
    }
}

fn exists(fs: &dyn Fs, loc: &Location) -> bool {
    match loc {
        Location::Local(p) => p.symlink_metadata().is_ok(),
        _ => fs.stat(&loc.backend_path()).is_ok(),
    }
}

/// `name`, or `name (2)`, `name (3)`… — the first one `taken` says is free.
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let stem = vfs::stem(name);
    let ext = vfs::extension(name);
    for n in 2..10_000 {
        let candidate = if ext.is_empty() { format!("{stem} ({n})") } else { format!("{stem} ({n}).{ext}") };
        if !taken(&candidate) {
            return candidate;
        }
    }
    format!("{name}.{}", epoch_ms())
}

/// Total bytes and items under the sources (for the progress bar).
fn measure(state: &JobState, fs: &dyn Fs, loc: &Location, kind: Kind, size: u64, totals: &mut (u64, u64)) {
    totals.1 += 1;
    if kind != Kind::Dir {
        totals.0 += size;
        return;
    }
    if state.cancelled() {
        return;
    }
    if let Ok(children) = fs.list(&loc.backend_path()) {
        for c in children {
            let child = loc.join(&c.name);
            measure(state, fs, &child, c.kind, c.size, totals);
        }
    }
}

fn transfer(state: &JobState, sources: &[Location], dest: &Location, mode: Mode) {
    let dst_fs = match vfs::fs_for(dest) {
        Ok(fs) => fs,
        Err(e) => return state.error(&dest.name(), &e),
    };
    // Measure first, for the progress bar.
    state.update(|s| s.current = String::new());
    let mut plan = Vec::new();
    let mut totals = (0u64, 0u64);
    for src in sources {
        if dest.starts_with(src) && (mode == Mode::Move || dest != src) {
            state.error(&src.name(), &vfs::other("a folder can't go inside itself"));
            continue;
        }
        let src_fs = match vfs::fs_for(src) {
            Ok(f) => f,
            Err(e) => {
                state.error(&src.name(), &e);
                continue;
            }
        };
        let entry = match src_fs.stat(&src.backend_path()) {
            Ok(e) => e,
            Err(e) => {
                state.error(&src.name(), &e);
                continue;
            }
        };
        // A move that is a rename needs no measuring.
        if !(mode == Mode::Move && quick_move_possible(src, dest)) {
            measure(state, src_fs.as_ref(), src, entry.kind, entry.size, &mut totals);
        }
        plan.push((src.clone(), src_fs, entry.kind, entry.size));
    }
    state.update(|s| {
        s.total_bytes = totals.0;
        s.total_items = totals.1.max(plan.len() as u64);
    });
    let mut policy: Option<Conflict> = None;
    for (src, src_fs, kind, size) in plan {
        if state.cancelled() {
            break;
        }
        let name = src.name();
        let duplicate = mode == Mode::Copy && src.parent().as_ref() == Some(dest);
        let mut target = dest.join(&name);
        if duplicate {
            target = dest.join(&unique_name(&name, |n| exists(dst_fs.as_ref(), &dest.join(n))));
        } else if exists(dst_fs.as_ref(), &target) {
            let choice = match policy {
                Some(p) => p,
                None => {
                    let a = state.ask(Question { name: name.clone(), is_dir: kind == Kind::Dir });
                    if a.apply_to_all {
                        policy = Some(a.conflict);
                    }
                    a.conflict
                }
            };
            if state.cancelled() {
                break;
            }
            match choice {
                Conflict::Skip => {
                    state.update(|s| s.done_items += 1);
                    continue;
                }
                Conflict::KeepBoth => target = dest.join(&unique_name(&name, |n| exists(dst_fs.as_ref(), &dest.join(n)))),
                Conflict::Replace => {
                    // A folder onto a folder merges; anything else replaces.
                    let existing_dir = dst_fs.stat(&target.backend_path()).map(|e| e.kind == Kind::Dir).unwrap_or(false);
                    if !(existing_dir && kind == Kind::Dir) {
                        if let Err(e) = delete_tree(state, dst_fs.as_ref(), &target) {
                            state.error(&name, &e);
                            continue;
                        }
                    }
                }
            }
        }
        if mode == Mode::Move && quick_move_possible(&src, dest) && !exists(dst_fs.as_ref(), &target) {
            state.update(|s| s.current = name.clone());
            match src_fs.rename(&src.backend_path(), &target.backend_path()) {
                Ok(()) => {
                    state.update(|s| {
                        s.done_items += 1;
                        s.created.push(target.clone());
                    });
                    state.touch(src.parent());
                    continue;
                }
                Err(e) => log::info!("dexfiles: rename failed ({e}); copying instead"),
            }
        }
        let r = copy_tree(state, src_fs.as_ref(), &src, kind, size, dst_fs.as_ref(), &target);
        match r {
            Ok(()) => {
                state.update(|s| s.created.push(target.clone()));
                if mode == Mode::Move && !state.cancelled() {
                    if let Err(e) = delete_tree(state, src_fs.as_ref(), &src) {
                        state.error(&name, &e);
                    }
                    state.touch(src.parent());
                }
            }
            Err(e) if vfs::is_cancelled(&e) => {}
            Err(e) => state.error(&name, &e),
        }
    }
    state.touch(Some(dest.clone()));
    let written: Vec<PathBuf> = state.status().created.iter().filter_map(|l| l.as_local().map(PathBuf::from)).collect();
    crate::platform::media_changed(&written);
}

/// Same device (local) or same server: a rename moves without copying.
fn quick_move_possible(src: &Location, dest: &Location) -> bool {
    match (src, dest) {
        (Location::Local(a), Location::Local(b)) => {
            let da = a.parent().and_then(crate::vfs::local::device);
            da.is_some() && da == crate::vfs::local::device(b)
        }
        _ => src.same_backend(dest),
    }
}

fn copy_tree(state: &JobState, src_fs: &dyn Fs, src: &Location, kind: Kind, size: u64, dst_fs: &dyn Fs, dst: &Location) -> io::Result<()> {
    if state.cancelled() {
        return Err(vfs::cancelled());
    }
    state.update(|s| s.current = src.name());
    if kind == Kind::Dir {
        if !exists(dst_fs, dst) {
            dst_fs.mkdir(&dst.backend_path())?;
        }
        state.update(|s| s.done_items += 1);
        let mut first_error = None;
        for child in src_fs.list(&src.backend_path())? {
            let r = copy_tree(state, src_fs, &src.join(&child.name), child.kind, child.size, dst_fs, &dst.join(&child.name));
            match r {
                Err(e) if vfs::is_cancelled(&e) => return Err(e),
                Err(e) => {
                    state.error(&child.name, &e);
                    first_error.get_or_insert(e);
                }
                Ok(()) => {}
            }
        }
        return match first_error {
            Some(e) => Err(io::Error::new(e.kind(), "some items could not be copied")),
            None => Ok(()),
        };
    }
    if kind == Kind::Other {
        state.update(|s| s.done_items += 1);
        return Ok(());
    }
    let base = state.status().done_bytes;
    let mut progress = |n: u64| {
        state.update(|s| s.done_bytes = base + n);
        !state.cancelled()
    };
    copy_file(src_fs, src, size, dst_fs, dst, &mut progress)?;
    state.update(|s| {
        s.done_bytes = base + size;
        s.done_items += 1;
    });
    Ok(())
}

fn copy_file(src_fs: &dyn Fs, src: &Location, size: u64, dst_fs: &dyn Fs, dst: &Location, progress: vfs::Progress<'_>) -> io::Result<()> {
    match (src, dst) {
        (Location::Local(s), _) => {
            let mut file = std::fs::File::open(s)?;
            let mtime = file.metadata().and_then(|m| m.modified()).ok();
            if let Location::Local(d) = dst {
                // Write to a hidden partial file, then rename: an interrupted copy leaves no
                // half file under the real name.
                let partial = partial_path(d);
                let r = dst_fs.write_from(&partial.to_string_lossy(), &mut file, size, progress).and_then(|_| std::fs::rename(&partial, d));
                if r.is_err() {
                    let _ = std::fs::remove_file(&partial);
                }
                r?;
                if let (Some(t), Ok(f)) = (mtime, std::fs::File::options().write(true).open(d)) {
                    let _ = f.set_modified(t);
                }
                Ok(())
            } else {
                dst_fs.write_from(&dst.backend_path(), &mut file, size, progress).map(|_| ())
            }
        }
        (_, Location::Local(d)) => {
            let partial = partial_path(d);
            let mut file = std::fs::File::create(&partial)?;
            let r = src_fs.read_into(&src.backend_path(), &mut file, progress).and_then(|_| {
                drop(file);
                std::fs::rename(&partial, d)
            });
            if r.is_err() {
                let _ = std::fs::remove_file(&partial);
            }
            r
        }
        _ => {
            // Server to server: through a temporary file.
            let dir = crate::platform::cache_dir().join("transfer");
            std::fs::create_dir_all(&dir)?;
            let tmp = dir.join(format!("{}-{}", epoch_ms(), src.name()));
            let r = (|| {
                let mut file = std::fs::File::create(&tmp)?;
                let mut half = |n: u64| progress(n / 2);
                src_fs.read_into(&src.backend_path(), &mut file, &mut half)?;
                drop(file);
                let mut file = std::fs::File::open(&tmp)?;
                let mut rest = |n: u64| progress(size / 2 + n / 2);
                dst_fs.write_from(&dst.backend_path(), &mut file, size, &mut rest).map(|_| ())
            })();
            let _ = std::fs::remove_file(&tmp);
            r
        }
    }
}

fn partial_path(target: &std::path::Path) -> PathBuf {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    target.with_file_name(format!(".{name}.dexfiles-part"))
}

fn delete_tree(state: &JobState, fs: &dyn Fs, loc: &Location) -> io::Result<()> {
    if let Location::Local(p) = loc {
        let meta = std::fs::symlink_metadata(p)?;
        return if meta.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
    }
    if state.cancelled() {
        return Err(vfs::cancelled());
    }
    let entry = fs.stat(&loc.backend_path())?;
    if entry.kind == Kind::Dir {
        for child in fs.list(&loc.backend_path())? {
            delete_tree(state, fs, &loc.join(&child.name))?;
        }
        fs.remove_dir(&loc.backend_path())
    } else {
        fs.remove_file(&loc.backend_path())
    }
}

/// Wait a little for a quick job (tests).
#[cfg(test)]
pub fn wait(job: &Job) -> Status {
    for _ in 0..500 {
        let s = job.state.status();
        if s.finished {
            return s;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    job.state.status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names() {
        let taken = ["a.txt", "a (2).txt", "dir"];
        assert_eq!(unique_name("b.txt", |n| taken.contains(&n)), "b.txt");
        assert_eq!(unique_name("a.txt", |n| taken.contains(&n)), "a (3).txt");
        assert_eq!(unique_name("dir", |n| taken.contains(&n)), "dir (2)");
    }

    #[test]
    fn copy_move_and_delete_locally() {
        let ctx = egui::Context::default();
        let root = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let a = root.path().join("a");
        let b = root.path().join("b");
        std::fs::create_dir_all(a.join("sub")).unwrap_or_default();
        std::fs::create_dir_all(&b).unwrap_or_default();
        std::fs::write(a.join("sub/one.txt"), b"hello").unwrap_or_default();
        std::fs::write(a.join("two.bin"), vec![7u8; 300_000]).unwrap_or_default();

        let s = wait(&start(&ctx, JobKind::Transfer { sources: vec![Location::local(&a)], dest: Location::local(&b), mode: Mode::Copy }));
        assert!(s.errors.is_empty(), "{:?}", s.errors);
        assert_eq!(std::fs::read(b.join("a/sub/one.txt")).unwrap_or_default(), b"hello");
        assert_eq!(std::fs::metadata(b.join("a/two.bin")).map(|m| m.len()).unwrap_or(0), 300_000);
        assert_eq!(s.total_bytes, 300_005);

        // Duplicating in place keeps both.
        let s = wait(&start(&ctx, JobKind::Transfer { sources: vec![Location::local(a.join("two.bin"))], dest: Location::local(&a), mode: Mode::Copy }));
        assert!(s.errors.is_empty());
        assert!(a.join("two (2).bin").exists());

        // Moving onto a taken name asks; answer "keep both".
        let job = start(&ctx, JobKind::Transfer { sources: vec![Location::local(a.join("two.bin"))], dest: Location::local(b.join("a")), mode: Mode::Move });
        for _ in 0..200 {
            if job.state.status().question.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(job.state.status().question.is_some());
        job.state.answer(Answer { conflict: Conflict::KeepBoth, apply_to_all: false });
        let s = wait(&job);
        assert!(s.errors.is_empty());
        assert!(!a.join("two.bin").exists());
        assert!(b.join("a/two (2).bin").exists());

        // A folder can't be copied into itself.
        let s = wait(&start(&ctx, JobKind::Transfer { sources: vec![Location::local(&a)], dest: Location::local(a.join("sub")), mode: Mode::Move }));
        assert_eq!(s.errors.len(), 1);

        let s = wait(&start(&ctx, JobKind::Delete { items: vec![Location::local(&b)] }));
        assert!(s.errors.is_empty());
        assert!(!b.exists());
    }
}
