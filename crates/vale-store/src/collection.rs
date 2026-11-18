//! Collections: named sets of layers for the Layers panel.

use std::collections::BTreeSet;

use crate::{Layer, LayerId, Project};

/// The identity of a collection. It is never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectionId(pub u32);

/// A named set of layers. A layer can be in more than one collection or in
/// none. A collection does not change the draw order.
#[derive(Clone, Debug, PartialEq)]
pub struct Collection {
    pub id: CollectionId,
    /// Unique in the project.
    pub name: String,
    pub layers: BTreeSet<LayerId>,
}

impl Project {
    /// The collections in the order of creation.
    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }

    pub fn collection(&self, id: CollectionId) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }

    /// "Collection N", with the first N that no collection has.
    pub fn free_collection_name(&self) -> String {
        (1..)
            .map(|n| format!("Collection {n}"))
            .find(|name| self.collections.iter().all(|c| &c.name != name))
            .expect("a free name")
    }

    /// True if `name` with no spaces at the ends is not blank and no
    /// collection other than `except` has it.
    pub fn collection_name_is_free(&self, name: &str, except: Option<CollectionId>) -> bool {
        let name = name.trim();
        !name.is_empty()
            && self
                .collections
                .iter()
                .all(|c| Some(c.id) == except || c.name != name)
    }

    /// Adds an empty collection after the others. `None` if the name is
    /// blank or another collection has it.
    pub fn add_collection(&mut self, name: &str) -> Option<CollectionId> {
        if !self.collection_name_is_free(name, None) {
            return None;
        }
        let id = CollectionId(self.next_collection_id);
        self.next_collection_id += 1;
        self.collections.push(Collection {
            id,
            name: name.trim().to_owned(),
            layers: BTreeSet::new(),
        });
        Some(id)
    }

    /// False if the collection is missing, or if the name is blank or
    /// another collection has it.
    pub fn rename_collection(&mut self, id: CollectionId, name: &str) -> bool {
        if !self.collection_name_is_free(name, Some(id)) {
            return false;
        }
        match self.collections.iter_mut().find(|c| c.id == id) {
            Some(c) => {
                c.name = name.trim().to_owned();
                true
            }
            None => false,
        }
    }

    /// Removes a collection and keeps its layers. If the Layers panel shows
    /// the collection, it shows all layers.
    pub fn remove_collection(&mut self, id: CollectionId) -> bool {
        let before = self.collections.len();
        self.collections.retain(|c| c.id != id);
        if self.shown_collection == Some(id) {
            self.shown_collection = None;
        }
        self.collections.len() != before
    }

    /// Puts a layer in a collection or takes it out. False if the
    /// collection or the layer is missing, or if nothing changes.
    pub fn set_in_collection(&mut self, id: CollectionId, layer: LayerId, on: bool) -> bool {
        if self.layer(layer).is_none() {
            return false;
        }
        let Some(c) = self.collections.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        if on {
            c.layers.insert(layer)
        } else {
            c.layers.remove(&layer)
        }
    }

    /// The collections that hold a layer, in the order of creation.
    pub fn collections_of(&self, layer: LayerId) -> Vec<&Collection> {
        self.collections
            .iter()
            .filter(|c| c.layers.contains(&layer))
            .collect()
    }

    /// The collection that the Layers panel shows, or `None` for all layers.
    pub fn shown_collection(&self) -> Option<CollectionId> {
        self.shown_collection
    }

    /// False for a missing collection. A change of the shown collection is
    /// not an edit.
    pub fn set_shown_collection(&mut self, id: Option<CollectionId>) -> bool {
        if id.is_some_and(|id| self.collection(id).is_none()) {
            return false;
        }
        self.shown_collection = id;
        true
    }

    /// The layers that the Layers panel lists, in the draw order.
    pub fn shown_layers(&self) -> Vec<&Layer> {
        let members = self
            .shown_collection
            .and_then(|id| self.collection(id))
            .map(|c| &c.layers);
        self.layers
            .iter()
            .filter(|l| members.is_none_or(|m| m.contains(&l.id)))
            .collect()
    }

    /// A new layer ID. The layer joins the shown collection, so that the
    /// Layers panel lists it.
    pub(crate) fn new_layer_id(&mut self) -> LayerId {
        let id = LayerId(self.next_id);
        self.next_id += 1;
        if let Some(shown) = self.shown_collection
            && let Some(c) = self.collections.iter_mut().find(|c| c.id == shown)
        {
            c.layers.insert(id);
        }
        id
    }

    /// Removes the members that are not layers, and shows all layers if the
    /// shown collection is missing.
    pub(crate) fn prune_collections(&mut self) {
        let ids: BTreeSet<LayerId> = self.layers.iter().map(|l| l.id).collect();
        for c in &mut self.collections {
            c.layers.retain(|id| ids.contains(id));
        }
        if self
            .shown_collection
            .is_some_and(|id| self.collection(id).is_none())
        {
            self.shown_collection = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{GeometryKind, Project, World};

    fn project() -> Project {
        let mut p = Project::new(World::default());
        for name in ["a", "b", "c"] {
            p.add_layer(name.into(), GeometryKind::Point, None, Vec::new());
        }
        p
    }

    #[test]
    fn names_are_trimmed_and_unique() {
        let mut p = project();
        let a = p.add_collection("  Rivers ").unwrap();
        assert_eq!(p.collection(a).unwrap().name, "Rivers");
        assert_eq!(p.add_collection("Rivers"), None);
        assert_eq!(p.add_collection("  "), None);
        let b = p.add_collection(&p.free_collection_name()).unwrap();
        assert_eq!(p.collection(b).unwrap().name, "Collection 1");
        assert!(!p.rename_collection(b, "Rivers"));
        assert!(p.rename_collection(a, "Rivers"));
        assert!(p.rename_collection(a, "Roads"));
    }

    #[test]
    fn shown_layers_keep_the_draw_order() {
        let mut p = project();
        let ids: Vec<_> = p.layers().iter().map(|l| l.id).collect();
        let c = p.add_collection("C").unwrap();
        p.set_in_collection(c, ids[2], true);
        p.set_in_collection(c, ids[0], true);
        assert_eq!(p.shown_layers().len(), 3);
        p.set_shown_collection(Some(c));
        let shown: Vec<_> = p.shown_layers().iter().map(|l| l.id).collect();
        assert_eq!(shown, vec![ids[0], ids[2]]);
    }

    #[test]
    fn a_new_layer_joins_the_shown_collection() {
        let mut p = project();
        let c = p.add_collection("C").unwrap();
        p.set_shown_collection(Some(c));
        let id = p.add_layer("d".into(), GeometryKind::Line, None, Vec::new());
        assert!(p.collection(c).unwrap().layers.contains(&id));
    }

    #[test]
    fn a_removed_collection_keeps_its_layers_and_shows_all_layers() {
        let mut p = project();
        let id = p.layers()[0].id;
        let c = p.add_collection("C").unwrap();
        p.set_in_collection(c, id, true);
        p.set_shown_collection(Some(c));
        assert!(p.remove_collection(c));
        assert_eq!(p.shown_collection(), None);
        assert_eq!(p.layers().len(), 3);
    }

    #[test]
    fn a_removed_layer_leaves_each_collection() {
        let mut p = project();
        let id = p.layers()[0].id;
        let a = p.add_collection("A").unwrap();
        let b = p.add_collection("B").unwrap();
        p.set_in_collection(a, id, true);
        p.set_in_collection(b, id, true);
        assert_eq!(p.collections_of(id).len(), 2);
        p.remove_layer(id);
        assert!(p.collections().iter().all(|c| c.layers.is_empty()));
    }
}
