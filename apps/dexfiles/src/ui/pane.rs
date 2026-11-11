//! Panes and their tabs: where each tab is, its history, what it lists (or found), sorted and
//! filtered, and what is selected.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::fileinfo::Category;
use crate::search::{Query, Search};
use crate::settings::{SortKey, ViewMode};
use crate::vfs::{Entry, Location};

static NEXT_TAB: AtomicU64 = AtomicU64::new(1);

pub struct Tab {
    pub id: u64,
    pub history: Vec<Location>,
    pub pos: usize,
    pub entries: Vec<Entry>,
    /// Indices into the source (entries or search results), sorted and filtered.
    pub visible: Vec<usize>,
    pub dirty: bool,
    pub loading: bool,
    pub error: Option<String>,
    /// Request generation, to drop stale listings.
    pub generation: u64,
    pub selected: HashSet<Location>,
    pub anchor: Option<usize>,
    pub cursor: Option<usize>,
    pub scroll_to_cursor: bool,
    /// Quick filter of the current folder (the search field without "search below").
    pub filter: String,
    pub query: Query,
    pub search: Option<Search>,
    pub search_results: Vec<Entry>,
    /// When to start the search typed in the field (a little after the last key).
    pub search_due: Option<std::time::Instant>,
    pub view: Option<ViewMode>,
    /// Select these once the next listing arrives (a new folder, pasted files).
    pub pending_select: Vec<Location>,
    pub pending_rename: Option<Location>,
    /// The folder's modification time when listed (local folders are re-listed when it changes).
    pub dir_stamp: Option<std::time::SystemTime>,
    pub last_check: std::time::Instant,
}

impl Tab {
    pub fn new(location: Location) -> Self {
        Tab {
            id: NEXT_TAB.fetch_add(1, AtomicOrdering::Relaxed),
            history: vec![location],
            pos: 0,
            entries: Vec::new(),
            visible: Vec::new(),
            dirty: true,
            loading: false,
            error: None,
            generation: 0,
            selected: HashSet::new(),
            anchor: None,
            cursor: None,
            scroll_to_cursor: false,
            filter: String::new(),
            query: Query::default(),
            search: None,
            search_results: Vec::new(),
            search_due: None,
            view: None,
            pending_select: Vec::new(),
            pending_rename: None,
            dir_stamp: None,
            last_check: std::time::Instant::now(),
        }
    }

    pub fn location(&self) -> &Location {
        &self.history[self.pos.min(self.history.len() - 1)]
    }

    pub fn title(&self) -> String {
        if self.search.is_some() {
            return format!("{} “{}”", crate::i18n::t("search"), self.query.text.trim());
        }
        location_title(self.location())
    }

    /// Go somewhere new (forgetting the forward history). Returns whether the place changed.
    pub fn navigate(&mut self, location: Location) -> bool {
        if *self.location() == location && self.search.is_none() {
            return false;
        }
        self.history.truncate(self.pos + 1);
        self.history.push(location);
        if self.history.len() > 100 {
            self.history.remove(0);
        }
        self.pos = self.history.len() - 1;
        self.reset();
        true
    }

    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.history.len()
    }

    pub fn back(&mut self) -> bool {
        if !self.can_back() {
            return false;
        }
        let from = self.location().clone();
        self.pos -= 1;
        self.reset();
        // Coming back up selects the folder we were in.
        self.pending_select = vec![from];
        true
    }

    pub fn forward(&mut self) -> bool {
        if !self.can_forward() {
            return false;
        }
        self.pos += 1;
        self.reset();
        true
    }

    fn reset(&mut self) {
        self.entries.clear();
        self.visible.clear();
        self.selected.clear();
        self.anchor = None;
        self.cursor = None;
        self.error = None;
        self.filter.clear();
        self.query.text.clear();
        self.search = None;
        self.search_results.clear();
        self.dirty = true;
        self.dir_stamp = None;
    }

    pub fn source(&self) -> &[Entry] {
        if self.search.is_some() {
            &self.search_results
        } else {
            &self.entries
        }
    }

    pub fn item(&self, visible_index: usize) -> Option<&Entry> {
        self.visible
            .get(visible_index)
            .and_then(|&i| self.source().get(i))
    }

    pub fn items(&self) -> impl Iterator<Item = &Entry> {
        self.visible.iter().filter_map(|&i| self.source().get(i))
    }

    pub fn selected_entries(&self) -> Vec<Entry> {
        self.items()
            .filter(|e| self.selected.contains(&e.location))
            .cloned()
            .collect()
    }

    pub fn selected_locations(&self) -> Vec<Location> {
        self.items()
            .filter(|e| self.selected.contains(&e.location))
            .map(|e| e.location.clone())
            .collect()
    }

    /// Re-sort and re-filter; `size_of` gives the size to sort by (folders: their contents').
    pub fn refresh_visible(
        &mut self,
        sort: SortKey,
        descending: bool,
        folders_first: bool,
        show_hidden: bool,
        size_of: &dyn Fn(&Entry) -> u64,
    ) {
        let filter = self.filter.trim().to_owned();
        let source = self.source();
        let mut visible: Vec<usize> = (0..source.len())
            .filter(|&i| {
                let e = &source[i];
                (show_hidden || !e.hidden || self.search.is_some())
                    && (filter.is_empty() || crate::search::name_matches(&e.name, &filter))
            })
            .collect();
        visible.sort_by(|&a, &b| compare(&source[a], &source[b], sort, descending, folders_first, size_of));
        self.visible = visible;
        self.dirty = false;
        // Keep only what is still there selected.
        let present: HashSet<&Location> = self.items().map(|e| &e.location).collect();
        let keep: HashSet<Location> = self
            .selected
            .iter()
            .filter(|l| present.contains(l))
            .cloned()
            .collect();
        self.selected = keep;
        self.cursor = self.cursor.filter(|&c| c < self.visible.len());
        if !self.pending_select.is_empty() {
            let wanted = std::mem::take(&mut self.pending_select);
            let mut first = None;
            for (i, e) in self.items().enumerate() {
                if wanted.contains(&e.location) {
                    first.get_or_insert(i);
                }
            }
            if first.is_some() {
                self.selected = wanted.into_iter().collect();
                self.cursor = first;
                self.anchor = first;
                self.scroll_to_cursor = true;
            } else if self.loading {
                self.pending_select = wanted;
            }
        }
    }

    pub fn select_only(&mut self, index: usize) {
        self.selected.clear();
        if let Some(e) = self.item(index) {
            let l = e.location.clone();
            self.selected.insert(l);
        }
        self.cursor = Some(index);
        self.anchor = Some(index);
    }

    pub fn toggle(&mut self, index: usize) {
        if let Some(l) = self.item(index).map(|e| e.location.clone())
            && !self.selected.remove(&l)
        {
            self.selected.insert(l);
        }
        self.cursor = Some(index);
        self.anchor = Some(index);
    }

    pub fn select_range(&mut self, index: usize, add: bool) {
        let anchor = self.anchor.unwrap_or(index);
        if !add {
            self.selected.clear();
        }
        let (a, b) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        for i in a..=b {
            if let Some(l) = self.item(i).map(|e| e.location.clone()) {
                self.selected.insert(l);
            }
        }
        self.cursor = Some(index);
    }

    pub fn select_all(&mut self) {
        let all: Vec<Location> = self.items().map(|e| e.location.clone()).collect();
        self.selected = all.into_iter().collect();
    }

}

pub fn location_title(l: &Location) -> String {
    match l {
        Location::Trash => crate::i18n::t("trash").to_owned(),
        Location::Local(p) => {
            if *p == crate::platform::home() {
                return crate::i18n::t("home").to_owned();
            }
            for v in crate::platform::volumes() {
                if v.path == *p {
                    return volume_label(&v);
                }
            }
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/".to_owned())
        }
        Location::Remote { server, path } => {
            let name = l.name();
            if name.is_empty() || path == "/" {
                crate::vfs::servers::server(*server)
                    .map(|s| s.display_name())
                    .unwrap_or_else(|| "?".into())
            } else {
                name
            }
        }
    }
}

pub fn volume_label(v: &crate::platform::Volume) -> String {
    if !v.label.is_empty() {
        return v.label.clone();
    }
    if v.path == std::path::Path::new("/") {
        return crate::i18n::t("root").to_owned();
    }
    if v.path == crate::platform::home() {
        return crate::i18n::t("internal_storage").to_owned();
    }
    v.path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Names compared as people read them: "file 2" before "file 10", ignoring case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn status_rank(e: &Entry) -> u8 {
    u8::from(e.readonly) * 4 + u8::from(e.hidden) * 2 + u8::from(e.symlink)
}

pub fn compare(
    a: &Entry,
    b: &Entry,
    sort: SortKey,
    descending: bool,
    folders_first: bool,
    size_of: &dyn Fn(&Entry) -> u64,
) -> Ordering {
    if folders_first && a.is_dir() != b.is_dir() {
        return if a.is_dir() {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let by_name = || natural_cmp(&a.name, &b.name);
    let o = match sort {
        SortKey::Name => by_name(),
        SortKey::Modified => a.modified.cmp(&b.modified).then_with(by_name),
        SortKey::Size => size_of(a).cmp(&size_of(b)).then_with(by_name),
        SortKey::Kind => Category::of(&a.ext, a.is_dir())
            .cmp(&Category::of(&b.ext, b.is_dir()))
            .then_with(|| a.ext.cmp(&b.ext))
            .then_with(by_name),
        SortKey::Status => status_rank(a).cmp(&status_rank(b)).then_with(by_name),
    };
    if descending { o.reverse() } else { o }
}

pub struct Pane {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

impl Pane {
    pub fn new(location: Location) -> Self {
        Pane {
            tabs: vec![Tab::new(location)],
            active: 0,
        }
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        let i = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[i]
    }

    pub fn open_tab(&mut self, location: Location) {
        self.tabs.insert(self.active + 1, Tab::new(location));
        self.active += 1;
    }

    pub fn close_tab(&mut self, i: usize) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.tabs.remove(i);
        if self.active >= self.tabs.len() || self.active > i {
            self.active = self.active.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10.txt", "File2.txt", "file1.txt", "a", "B"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a", "B", "file1.txt", "File2.txt", "file10.txt"]);
    }

    #[test]
    fn history() {
        let mut t = Tab::new(Location::local("/a"));
        t.navigate(Location::local("/a/b"));
        t.navigate(Location::local("/a/b/c"));
        assert!(t.back());
        assert_eq!(t.location(), &Location::local("/a/b"));
        assert_eq!(t.pending_select, vec![Location::local("/a/b/c")]);
        assert!(t.forward());
        assert!(!t.forward());
        t.back();
        t.navigate(Location::local("/x"));
        assert!(!t.can_forward());
    }
}
