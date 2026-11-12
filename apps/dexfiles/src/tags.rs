//! Tags: any file or folder can carry some of the user's tags, each a name and a colour (seven
//! colours to start with; Settings ▸ Tags manages them). They are DexFiles' own, kept in its
//! settings by location and shown as dots on the icons; each tag is also a place
//! (`Location::Tag`) listing what carries it, wherever it is.

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use egui::Color32;

pub use crate::settings::TagDef;
use crate::vfs::{Entry, Location};

/// The colours DexFiles starts with (ids 0–6), and offers for new tags.
pub const PALETTE: [[u8; 3]; 7] = [
    [0xf2, 0x4b, 0x4b],
    [0xf5, 0x93, 0x2a],
    [0xf2, 0xc9, 0x2e],
    [0x4c, 0xc2, 0x5a],
    [0x2f, 0x8c, 0xf0],
    [0xa8, 0x5c, 0xe0],
    [0x8e, 0x8e, 0x93],
];

const BUILTIN: [&str; 7] = [
    "tag_red",
    "tag_orange",
    "tag_yellow",
    "tag_green",
    "tag_blue",
    "tag_purple",
    "tag_gray",
];

/// The tags DexFiles starts with: the seven colours, named after them (in the interface's
/// language at that moment; they are the user's to rename).
pub fn builtin() -> Vec<TagDef> {
    (0..7u8)
        .map(|id| TagDef {
            id,
            name: default_name(id),
            color: PALETTE[id as usize],
        })
        .collect()
}

/// The tags as defined in the settings (published by the interface, read anywhere).
static DEFS: RwLock<Vec<TagDef>> = RwLock::new(Vec::new());

pub fn publish_defs(defs: &[TagDef]) {
    *DEFS.write().unwrap_or_else(PoisonError::into_inner) = defs.to_vec();
}

/// The defined tags, in their order.
pub fn defs() -> Vec<TagDef> {
    DEFS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn def(c: u8) -> Option<TagDef> {
    DEFS.read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|d| d.id == c)
        .cloned()
}

pub fn color(c: u8) -> Color32 {
    let [r, g, b] = def(c)
        .map(|d| d.color)
        .unwrap_or(PALETTE[c as usize % PALETTE.len()]);
    Color32::from_rgb(r, g, b)
}

/// A tag's name: its own, or for the starting ones not renamed, their colour's.
pub fn name(c: u8) -> String {
    match def(c) {
        Some(d) if !d.name.trim().is_empty() => d.name,
        _ => default_name(c),
    }
}

pub fn default_name(c: u8) -> String {
    match BUILTIN.get(c as usize) {
        Some(key) => crate::i18n::t(key).to_owned(),
        None => format!("#{c}"),
    }
}

/// An id no tag uses (nor any item still carries).
pub fn free_id(defs: &[TagDef], tags: &HashMap<Location, Vec<u8>>) -> Option<u8> {
    (0..=u8::MAX)
        .find(|id| !defs.iter().any(|d| d.id == *id) && !tags.values().any(|v| v.contains(id)))
}

/// The tags as last published by the interface, for listing a colour off its thread.
static TAGS: RwLock<Vec<(Location, Vec<u8>)>> = RwLock::new(Vec::new());

pub fn publish(tags: &HashMap<Location, Vec<u8>>) {
    let list = tags.iter().map(|(l, c)| (l.clone(), c.clone())).collect();
    *TAGS.write().unwrap_or_else(PoisonError::into_inner) = list;
}

/// What carries colour `c` (that still exists). Blocking: servers may be asked.
pub fn list(c: u8) -> Vec<Entry> {
    let tagged: Vec<Location> = TAGS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|(_, cs)| cs.contains(&c))
        .map(|(l, _)| l.clone())
        .collect();
    tagged
        .iter()
        .filter_map(|l| crate::vfs::stat(l).ok())
        .collect()
}

/// After a move or rename: the tags of `from` and of what is inside it go along to `to`.
pub fn moved(tags: &mut HashMap<Location, Vec<u8>>, from: &Location, to: &Location) {
    let affected: Vec<Location> = tags
        .keys()
        .filter(|l| l.starts_with(from))
        .cloned()
        .collect();
    for l in affected {
        let Some(colors) = tags.remove(&l) else {
            continue;
        };
        let new = if l == *from {
            to.clone()
        } else {
            match (&l, from, to) {
                (Location::Local(p), Location::Local(f), Location::Local(t)) => {
                    match p.strip_prefix(f) {
                        Ok(rest) => Location::Local(t.join(rest)),
                        Err(_) => l.clone(),
                    }
                }
                (
                    Location::Remote { path, .. },
                    Location::Remote { path: f, .. },
                    Location::Remote { server, path: t },
                ) => Location::Remote {
                    server: *server,
                    path: format!(
                        "{}{}",
                        t.trim_end_matches('/'),
                        &path[f.trim_end_matches('/').len()..]
                    ),
                },
                _ => l.clone(),
            }
        };
        tags.insert(new, colors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_follow_moves() {
        let mut tags = HashMap::new();
        tags.insert(Location::local("/a/b"), vec![1]);
        tags.insert(Location::local("/a/b/c.txt"), vec![2, 4]);
        tags.insert(Location::local("/a/bc"), vec![3]);
        moved(
            &mut tags,
            &Location::local("/a/b"),
            &Location::local("/x/y"),
        );
        assert_eq!(tags.get(&Location::local("/x/y")), Some(&vec![1]));
        assert_eq!(tags.get(&Location::local("/x/y/c.txt")), Some(&vec![2, 4]));
        assert_eq!(tags.get(&Location::local("/a/bc")), Some(&vec![3]));
        assert_eq!(tags.len(), 3);
    }
}
