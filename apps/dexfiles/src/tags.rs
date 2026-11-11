//! Colour tags: any file or folder can carry some of seven colours. They are DexFiles' own (kept
//! in its settings, by location), shown as dots beside the names; each colour is also a place
//! (`Location::Tag`) listing what carries it, wherever it is.

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use egui::Color32;

use crate::vfs::{Entry, Location};

pub const COUNT: u8 = 7;

/// The colours, in the order they are offered.
pub fn color(c: u8) -> Color32 {
    match c {
        0 => Color32::from_rgb(0xf2, 0x4b, 0x4b),
        1 => Color32::from_rgb(0xf5, 0x93, 0x2a),
        2 => Color32::from_rgb(0xf2, 0xc9, 0x2e),
        3 => Color32::from_rgb(0x4c, 0xc2, 0x5a),
        4 => Color32::from_rgb(0x2f, 0x8c, 0xf0),
        5 => Color32::from_rgb(0xa8, 0x5c, 0xe0),
        _ => Color32::from_rgb(0x8e, 0x8e, 0x93),
    }
}

pub fn key(c: u8) -> &'static str {
    match c {
        0 => "tag_red",
        1 => "tag_orange",
        2 => "tag_yellow",
        3 => "tag_green",
        4 => "tag_blue",
        5 => "tag_purple",
        _ => "tag_gray",
    }
}

pub fn name(c: u8) -> String {
    crate::i18n::t(key(c)).to_owned()
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
