//! Snapshot-based undo/redo history.
//!
//! Each committed edit stores both the *before* and *after* document snapshots
//! (clones of the pure-data `Document`) plus the active layer before/after, so
//! undo and redo are exact. A byte budget keeps memory bounded by dropping the
//! oldest snapshots when the retained pixel data grows too large.

use crate::core::document::Document;
use crate::core::pixel_buffer::LayerId;

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    before: Document,
    after: Document,
    active_before: Option<LayerId>,
    active_after: Option<LayerId>,
}

/// Undo/redo manager. `cursor` is the number of entries currently applied; it
/// points into `entries` such that `entries[0..cursor]` are the applied states.
#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<Entry>,
    cursor: usize,
    byte_limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        History {
            entries: Vec::new(),
            cursor: 0,
            byte_limit: 4_000_000_000, // ~4 GB of retained snapshots
        }
    }

    pub fn with_byte_limit(byte_limit: usize) -> Self {
        History {
            byte_limit,
            ..Self::new()
        }
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    /// The name of the action that would be undone, if any.
    pub fn undo_name(&self) -> Option<&str> {
        (self.cursor > 0).then(|| self.entries[self.cursor - 1].name.as_str())
    }

    /// The name of the action that would be redone, if any.
    pub fn redo_name(&self) -> Option<&str> {
        (self.cursor < self.entries.len()).then(|| self.entries[self.cursor].name.as_str())
    }

    /// Record a committed edit. Drops any redo branch, appends the new entry,
    /// and trims old snapshots if the byte budget is exceeded.
    pub fn record(
        &mut self,
        name: &str,
        before: &Document,
        after: &Document,
        active_before: Option<LayerId>,
        active_after: Option<LayerId>,
    ) {
        // Drop the redo branch.
        self.entries.truncate(self.cursor);
        self.entries.push(Entry {
            name: name.to_string(),
            before: before.clone(),
            after: after.clone(),
            active_before,
            active_after,
        });
        self.cursor = self.entries.len();
        self.trim();
    }

    /// Step back one edit, returning the document and active layer to restore.
    pub fn undo(&mut self) -> Option<(Document, Option<LayerId>)> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        let e = &self.entries[self.cursor];
        Some((e.before.clone(), e.active_before))
    }

    /// Step forward one edit, returning the document and active layer to restore.
    pub fn redo(&mut self) -> Option<(Document, Option<LayerId>)> {
        if self.cursor >= self.entries.len() {
            return None;
        }
        let e = &self.entries[self.cursor];
        self.cursor += 1;
        Some((e.after.clone(), e.active_after))
    }

    /// Number of retained snapshots (for debugging / UI).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop oldest entries until the retained byte total is within budget.
    fn trim(&mut self) {
        let mut total = self.retained_bytes();
        while total > self.byte_limit && self.entries.len() > 1 {
            let removed = self.entries.remove(0);
            self.cursor -= 1;
            total -= doc_bytes(&removed.before) + doc_bytes(&removed.after);
        }
    }

    fn retained_bytes(&self) -> usize {
        self.entries
            .iter()
            .map(|e| doc_bytes(&e.before) + doc_bytes(&e.after))
            .sum()
    }
}

/// Approximate retained bytes of a document: every raster buffer it holds.
fn doc_bytes(doc: &Document) -> usize {
    let mut bytes = 0usize;
    for l in &doc.layers {
        if let Some(a) = &l.asset {
            bytes += a.image.byte_size() + a.thumbnail.byte_size();
        }
        if let Some(m) = &l.mask {
            bytes += m.asset.image.byte_size();
        }
        if let Some(s) = &l.shape {
            bytes += s.image.byte_size();
        }
        if let Some(t) = &l.text {
            bytes += t.image.byte_size();
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::layer::Layer;

    fn doc_with_layer() -> Document {
        let mut doc = Document::new(10, 10);
        doc.insert_layer(Layer::blank("A".into(), (10, 10)), 0);
        doc
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut hist = History::new();
        let d0 = doc_with_layer();
        let mut d1 = d0.clone();
        d1.layers[0].name = "B".into();
        hist.record("rename", &d0, &d1, None, None);
        assert!(hist.can_undo());

        let (restored, _) = hist.undo().unwrap();
        assert_eq!(restored.layers[0].name, "A");
        assert!(hist.can_redo());

        let (redone, _) = hist.redo().unwrap();
        assert_eq!(redone.layers[0].name, "B");
    }

    #[test]
    fn new_edit_drops_redo() {
        let mut hist = History::new();
        let d0 = doc_with_layer();
        let mut d1 = d0.clone();
        d1.layers[0].name = "B".into();
        hist.record("e1", &d0, &d1, None, None);
        let _ = hist.undo();

        let mut d2 = d0.clone();
        d2.layers[0].name = "C".into();
        hist.record("e2", &d0, &d2, None, None);
        assert!(!hist.can_redo());
    }
}
