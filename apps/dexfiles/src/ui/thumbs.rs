//! Thumbnails for the icon views: decoded by two worker threads (newest request first, so the
//! visible ones come before those scrolled past), kept as textures with a size limit.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::vfs::Entry;

pub const SIZE: u32 = 256;
const MAX_TEXTURES: usize = 600;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Key(String, Option<i64>, u64);

impl Key {
    pub fn of(e: &Entry) -> Key {
        Key(format!("{:?}", e.location), e.modified, e.size)
    }
}

enum Slot {
    Pending,
    Ready(egui::TextureHandle, u64),
    Failed,
}

struct Queue {
    items: VecDeque<(Key, Entry)>,
    done: Vec<(Key, Option<egui::ColorImage>)>,
}

pub struct Thumbs {
    slots: HashMap<Key, Slot>,
    queue: Arc<(Mutex<Queue>, Condvar)>,
    frame: u64,
}

impl Thumbs {
    pub fn new(ctx: &egui::Context) -> Self {
        let queue = Arc::new((
            Mutex::new(Queue {
                items: VecDeque::new(),
                done: Vec::new(),
            }),
            Condvar::new(),
        ));
        for n in 0..2 {
            let q = queue.clone();
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new()
                .name(format!("dexfiles-thumb{n}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let (lock, cv) = &*q;
                            let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
                            while guard.items.is_empty() {
                                guard = cv.wait(guard).unwrap_or_else(PoisonError::into_inner);
                            }
                            guard.items.pop_back()
                        };
                        let Some((key, entry)) = job else { continue };
                        let image = crate::preview::thumbnail(&entry, SIZE);
                        q.0.lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .done
                            .push((key, image));
                        ctx.request_repaint();
                    }
                });
        }
        Thumbs {
            slots: HashMap::new(),
            queue,
            frame: 0,
        }
    }

    /// Upload what the workers finished; forget the least recently shown beyond the limit.
    pub fn update(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        let done = std::mem::take(
            &mut self
                .queue
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .done,
        );
        for (key, image) in done {
            let slot = match image {
                Some(img) => Slot::Ready(
                    ctx.load_texture(
                        format!("thumb-{:?}", key.0),
                        img,
                        egui::TextureOptions::LINEAR,
                    ),
                    self.frame,
                ),
                None => Slot::Failed,
            };
            self.slots.insert(key, slot);
        }
        let ready = self
            .slots
            .values()
            .filter(|s| matches!(s, Slot::Ready(..)))
            .count();
        if ready > MAX_TEXTURES {
            let mut ages: Vec<(u64, Key)> = self
                .slots
                .iter()
                .filter_map(|(k, s)| {
                    if let Slot::Ready(_, f) = s {
                        Some((*f, k.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            ages.sort_by_key(|(f, _)| *f);
            for (_, k) in ages.into_iter().take(ready - MAX_TEXTURES) {
                self.slots.remove(&k);
            }
        }
    }

    /// The thumbnail if ready; otherwise asks for it.
    pub fn get(&mut self, e: &Entry) -> Option<egui::TextureHandle> {
        let key = Key::of(e);
        match self.slots.get_mut(&key) {
            Some(Slot::Ready(t, f)) => {
                *f = self.frame;
                Some(t.clone())
            }
            Some(_) => None,
            None => {
                self.slots.insert(key.clone(), Slot::Pending);
                let (lock, cv) = &*self.queue;
                let mut q = lock.lock().unwrap_or_else(PoisonError::into_inner);
                q.items.push_back((key, e.clone()));
                // Too many waiting: drop the oldest (scrolled away).
                while q.items.len() > 200 {
                    if let Some((k, _)) = q.items.pop_front() {
                        self.slots.remove(&k);
                    }
                }
                cv.notify_one();
                None
            }
        }
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }
}
