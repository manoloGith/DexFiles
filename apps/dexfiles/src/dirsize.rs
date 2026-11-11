//! Folder sizes, computed in the background and cached.
//!
//! Asking for a folder's size queues it (newest first, so the folders on screen go before
//! those scrolled past). Two worker threads walk each queued folder once, bottom-up, and record
//! the size of every subfolder on the way, so opening one of them later costs nothing. A size is
//! kept for a while and dropped when an operation of the app touches the folder or something
//! inside it, or when the folder's own listing changes.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Sizes older than this are computed again when asked for.
const FRESH: Duration = Duration::from_secs(10 * 60);
/// Folders waiting beyond this many are forgotten (scrolled away long ago).
const MAX_QUEUE: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub bytes: u64,
    /// Files and folders inside, at any depth.
    pub items: u64,
}

enum Slot {
    Pending,
    Done(Size, Instant),
}

struct Shared {
    sizes: Mutex<HashMap<PathBuf, Slot>>,
    queue: Mutex<VecDeque<PathBuf>>,
    wake: Condvar,
    /// Bumped whenever a size arrives (lists sorted by size re-sort).
    generation: AtomicU64,
}

#[derive(Clone)]
pub struct DirSizes {
    shared: Arc<Shared>,
}

impl DirSizes {
    pub fn new(ctx: &egui::Context) -> Self {
        let shared = Arc::new(Shared {
            sizes: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            generation: AtomicU64::new(0),
        });
        for n in 0..2 {
            let shared = shared.clone();
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new().name(format!("dexfiles-size{n}")).spawn(move || worker(&shared, &ctx));
        }
        DirSizes { shared }
    }

    /// The size if known; otherwise it is queued and `None` comes back for now.
    pub fn get(&self, path: &Path) -> Option<Size> {
        let mut sizes = self.shared.sizes.lock().unwrap_or_else(PoisonError::into_inner);
        match sizes.get(path) {
            Some(Slot::Done(size, at)) if at.elapsed() < FRESH => return Some(*size),
            Some(Slot::Done(size, _)) => {
                // Stale: show it while it is computed again.
                let size = *size;
                sizes.insert(path.to_path_buf(), Slot::Pending);
                drop(sizes);
                self.enqueue(path);
                return Some(size);
            }
            Some(Slot::Pending) => return None,
            None => {}
        }
        sizes.insert(path.to_path_buf(), Slot::Pending);
        drop(sizes);
        self.enqueue(path);
        None
    }

    /// The size if known, without asking for it.
    pub fn peek(&self, path: &Path) -> Option<Size> {
        match self.shared.sizes.lock().unwrap_or_else(PoisonError::into_inner).get(path) {
            Some(Slot::Done(size, _)) => Some(*size),
            _ => None,
        }
    }

    fn enqueue(&self, path: &Path) {
        let mut q = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        q.retain(|p| p != path);
        q.push_back(path.to_path_buf());
        while q.len() > MAX_QUEUE {
            if let Some(old) = q.pop_front() {
                self.shared.sizes.lock().unwrap_or_else(PoisonError::into_inner).remove(&old);
            }
        }
        self.shared.wake.notify_one();
    }

    /// Something changed in `path`: forget its size and those of the folders above it.
    pub fn invalidate(&self, path: &Path) {
        let mut sizes = self.shared.sizes.lock().unwrap_or_else(PoisonError::into_inner);
        let mut cur = Some(path);
        while let Some(p) = cur {
            if matches!(sizes.get(p), Some(Slot::Done(..))) {
                sizes.remove(p);
            }
            cur = p.parent();
        }
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Relaxed)
    }
}

fn worker(shared: &Shared, ctx: &egui::Context) {
    loop {
        let path = {
            let mut q = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            while q.is_empty() {
                q = shared.wake.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
            q.pop_back()
        };
        let Some(path) = path else { continue };
        // Another worker may have measured it as part of a bigger folder meanwhile.
        if matches!(shared.sizes.lock().unwrap_or_else(PoisonError::into_inner).get(&path), Some(Slot::Done(_, at)) if at.elapsed() < FRESH) {
            continue;
        }
        let mut found = Vec::new();
        let size = measure(&path, &mut found);
        let now = Instant::now();
        {
            let mut sizes = shared.sizes.lock().unwrap_or_else(PoisonError::into_inner);
            for (p, s) in found {
                sizes.insert(p, Slot::Done(s, now));
            }
            sizes.insert(path, Slot::Done(size, now));
        }
        shared.generation.fetch_add(1, Ordering::Relaxed);
        ctx.request_repaint();
    }
}

/// The size of `root`, walking it bottom-up without recursion (deep trees are fine); the size
/// of every subfolder goes to `found`. Links are not followed; unreadable folders count as empty.
pub fn measure(root: &Path, found: &mut Vec<(PathBuf, Size)>) -> Size {
    struct Frame {
        path: PathBuf,
        children: Vec<PathBuf>,
        next: usize,
        size: Size,
    }
    fn open(path: PathBuf) -> Frame {
        let mut size = Size { bytes: 0, items: 0 };
        let mut children = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&path) {
            for item in rd.flatten() {
                let Ok(ft) = item.file_type() else { continue };
                size.items += 1;
                if ft.is_dir() {
                    children.push(item.path());
                } else if ft.is_file() {
                    size.bytes += item.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }
        Frame { path, children, next: 0, size }
    }
    let mut stack = vec![open(root.to_path_buf())];
    loop {
        let Some(top) = stack.last_mut() else { return Size { bytes: 0, items: 0 } };
        if top.next < top.children.len() {
            let child = top.children[top.next].clone();
            top.next += 1;
            stack.push(open(child));
            continue;
        }
        let Some(done) = stack.pop() else { return Size { bytes: 0, items: 0 } };
        match stack.last_mut() {
            Some(parent) => {
                parent.size.bytes += done.size.bytes;
                parent.size.items += done.size.items;
                found.push((done.path, done.size));
            }
            None => return done.size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_add_up_bottom_up() {
        let root = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let r = root.path();
        std::fs::create_dir_all(r.join("a/b/c")).unwrap_or_default();
        std::fs::write(r.join("top.bin"), vec![0u8; 10]).unwrap_or_default();
        std::fs::write(r.join("a/x.bin"), vec![0u8; 100]).unwrap_or_default();
        std::fs::write(r.join("a/b/c/y.bin"), vec![0u8; 1000]).unwrap_or_default();
        let mut found = Vec::new();
        let s = measure(r, &mut found);
        assert_eq!(s.bytes, 1110);
        assert_eq!(s.items, 6); // top.bin, a, x.bin, b, c, y.bin
        let b = found.iter().find(|(p, _)| p.ends_with("a/b")).map(|(_, s)| *s);
        assert_eq!(b, Some(Size { bytes: 1000, items: 2 }));
    }
}
