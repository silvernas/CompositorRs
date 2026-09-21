//! The document: canvas size, the layer list, and tree/ordering helpers.
//!
//! Layers live in a single `Vec` ordered bottom-to-top. Folders are just layers
//! with `is_group == true` and no asset; their children reference them via
//! `parent_id`. Folders are *pass-through* (their children composite straight
//! onto what is below), so the draw order is simply the list order with a
//! visibility check — but a folder's mask still clips each of its children.

use crate::core::layer::Layer;
use crate::core::pixel_buffer::LayerId;
use crate::core::selection::Selection;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Debug)]
pub struct Document {
    pub id: LayerId,
    pub width: u32,
    pub height: u32,
    pub resolution: f32,
    /// Bottom-to-top layer stack.
    pub layers: Vec<Layer>,
    /// `None` = no selection (edits touch everything); `Some` = coverage mask.
    pub selection: Option<Selection>,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Self {
        Document {
            id: LayerId::new(),
            width,
            height,
            resolution: 72.0,
            layers: Vec::new(),
            selection: None,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Validate a dimension string (1..=30_000), mirroring `CanvasDocument.validDimension`.
    pub fn valid_dimension(value: &str) -> Option<u32> {
        value.trim().parse::<u32>().ok().filter(|&n| (1..=30_000).contains(&n))
    }

    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Layers with no parent, in stack order.
    pub fn top_level(&self) -> Vec<LayerId> {
        self.layers
            .iter()
            .filter(|l| l.parent_id.is_none())
            .map(|l| l.id)
            .collect()
    }

    /// Direct children of a group, in stack order.
    pub fn children_of(&self, group: LayerId) -> Vec<LayerId> {
        self.layers
            .iter()
            .filter(|l| l.parent_id == Some(group))
            .map(|l| l.id)
            .collect()
    }

    /// Immediate parent of a layer.
    pub fn parent_of(&self, id: LayerId) -> Option<LayerId> {
        self.layer(id).and_then(|l| l.parent_id)
    }

    /// The chain of ancestor groups from the immediate parent up to the root.
    pub fn ancestor_ids(&self, mut id: LayerId) -> Vec<LayerId> {
        let mut chain = Vec::new();
        while let Some(parent) = self.parent_of(id) {
            chain.push(parent);
            id = parent;
        }
        chain
    }

    /// All descendants of a group (not including the group itself).
    pub fn descendant_ids(&self, group: LayerId) -> Vec<LayerId> {
        let mut result = Vec::new();
        let mut stack = vec![group];
        while let Some(current) = stack.pop() {
            for child in self.children_of(current) {
                result.push(child);
                if self.layer(child).map(|l| l.is_group).unwrap_or(false) {
                    stack.push(child);
                }
            }
        }
        result
    }

    /// A layer is effectively visible only if it and every ancestor is visible.
    pub fn is_effective_visible(&self, id: LayerId) -> bool {
        let mut current = Some(id);
        while let Some(c) = current {
            match self.layer(c) {
                Some(l) if l.is_visible => current = l.parent_id,
                _ => return false,
            }
        }
        true
    }

    /// The draw order (bottom-to-top) of layers that should actually be painted:
    /// every layer in list order, skipping those that are not effectively visible.
    /// Groups contribute no pixels themselves but their masks clip descendants
    /// (the compositor reads `ancestor_ids` for that).
    pub fn paint_order(&self) -> Vec<LayerId> {
        self.layers
            .iter()
            .filter(|l| self.is_effective_visible(l.id))
            .map(|l| l.id)
            .collect()
    }

    /// Insert a layer at a stack index (clamped).
    pub fn insert_layer(&mut self, layer: Layer, index: usize) {
        let index = index.min(self.layers.len());
        self.layers.insert(index, layer);
    }

    /// Remove a layer and all of its descendants.
    pub fn remove_layer(&mut self, id: LayerId) {
        let descendants: Vec<LayerId> = if self.layer(id).map(|l| l.is_group).unwrap_or(false) {
            self.descendant_ids(id)
        } else {
            Vec::new()
        };
        let to_remove: std::collections::HashSet<LayerId> =
            descendants.into_iter().chain(std::iter::once(id)).collect();
        self.layers.retain(|l| !to_remove.contains(&l.id));
    }

    /// Move `id` to be a child of `new_parent` (or top-level when `None`),
    /// keeping it after the parent in stack order.
    pub fn reparent(&mut self, id: LayerId, new_parent: Option<LayerId>) {
        if let Some(l) = self.layer_mut(id) {
            l.parent_id = new_parent;
        }
    }

    /// Clone a set of root layers (with their descendants), assigning fresh ids
    /// and remapping `parent_id` / `mask_source_id` within the copied set. Returns
    /// the new layers and a map from old → new id. Mirrors Swift's `copyLayer`.
    pub fn duplicate_subtree(&self, roots: &[LayerId]) -> (Vec<Layer>, HashMap<LayerId, LayerId>) {
        let mut included: std::collections::HashSet<LayerId> = roots.iter().copied().collect();
        for &root in roots {
            if self.layer(root).map(|l| l.is_group).unwrap_or(false) {
                for d in self.descendant_ids(root) {
                    included.insert(d);
                }
            }
        }
        let mut mapping = HashMap::new();
        let mut new_layers = Vec::new();
        for layer in self.layers.iter().filter(|l| included.contains(&l.id)) {
            let new_id = LayerId::new();
            mapping.insert(layer.id, new_id);
            new_layers.push(layer.clone_with_id(new_id));
        }
        // Remap parent_id and mask_source_id that point inside the copied set.
        for layer in &mut new_layers {
            if let Some(p) = layer.parent_id {
                if let Some(&m) = mapping.get(&p) {
                    layer.parent_id = Some(m);
                }
            }
            if let Some(m) = layer.mask_source_id {
                if let Some(&n) = mapping.get(&m) {
                    layer.mask_source_id = Some(n);
                }
            }
        }
        (new_layers, mapping)
    }

    /// Total pixel budget currently used by layer assets + masks (for the
    /// 100-megapixel import limit check).
    pub fn used_pixels(&self) -> u64 {
        let mut total = 0u64;
        for l in &self.layers {
            if let Some(a) = &l.asset {
                total += a.image.pixel_count() as u64;
            }
            if let Some(m) = &l.mask {
                total += m.asset.image.pixel_count() as u64;
            }
        }
        total
    }
}

impl Layer {
    /// Clone this layer with a new id (used by `duplicate_subtree`).
    pub(crate) fn clone_with_id(&self, new_id: LayerId) -> Layer {
        Layer {
            id: new_id,
            asset: self.asset.clone(),
            transform: self.transform,
            name: self.name.clone(),
            is_visible: self.is_visible,
            parent_id: self.parent_id,
            is_group: self.is_group,
            opacity: self.opacity,
            blend_mode: self.blend_mode,
            mask_source_id: self.mask_source_id,
            mask: self.mask.clone(),
            adjustment: self.adjustment.clone(),
            shape: self.shape.clone(),
            effects: self.effects.clone(),
            text: self.text.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::layer::Asset;

    fn blank_doc() -> (Document, LayerId, LayerId, LayerId) {
        let mut doc = Document::new(100, 100);
        let base = Layer::blank("Base".into(), (100, 100));
        let base_id = base.id;
        let group = Layer::group("Folder".into());
        let group_id = group.id;
        let child = Layer::blank("Child".into(), (50, 50));
        let child_id = child.id;
        doc.insert_layer(base, 0);
        doc.insert_layer(group, 1);
        doc.insert_layer(child, 2);
        doc.reparent(child_id, Some(group_id));
        (doc, base_id, group_id, child_id)
    }

    #[test]
    fn tree_helpers() {
        let (doc, _b, group, child) = blank_doc();
        assert_eq!(doc.children_of(group), vec![child]);
        assert_eq!(doc.ancestor_ids(child), vec![group]);
        assert!(doc.is_effective_visible(child));
    }

    #[test]
    fn remove_group_removes_children() {
        let (mut doc, _b, group, child) = blank_doc();
        doc.remove_layer(group);
        assert!(doc.layer(group).is_none());
        assert!(doc.layer(child).is_none());
        assert_eq!(doc.layers.len(), 1);
    }

    #[test]
    fn duplicate_remaps_ids() {
        let (doc, _b, group, child) = blank_doc();
        let (copies, mapping) = doc.duplicate_subtree(&[group]);
        assert_eq!(copies.len(), 2);
        let new_group = mapping[&group];
        let new_child = copies.iter().find(|l| l.parent_id == Some(new_group)).unwrap();
        assert_eq!(new_child.id, mapping[&child]);
        assert_ne!(new_group, group);
    }
}
