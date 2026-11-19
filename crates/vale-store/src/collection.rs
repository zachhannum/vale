//! Collections: named, ordered stacks of layers.

use std::collections::BTreeSet;

use crate::{Layer, LayerId, MapId, Project};

/// The identity of a collection. It is never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectionId(pub u32);

/// The name of World in a new project.
pub const WORLD_COLLECTION: &str = "World";

/// A named stack of layers. A layer can be in more than one collection, and
/// each layer is in at least one collection.
#[derive(Clone, Debug, PartialEq)]
pub struct Collection {
    pub id: CollectionId,
    /// Unique in the project.
    pub name: String,
    /// From the bottom to the top, with no layer twice.
    pub layers: Vec<LayerId>,
}

impl Project {
    /// The collections in the order of creation, with World first.
    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }

    pub fn collection(&self, id: CollectionId) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }

    fn collection_mut(&mut self, id: CollectionId) -> Option<&mut Collection> {
        self.collections.iter_mut().find(|c| c.id == id)
    }

    /// World, the default collection. It cannot be removed.
    pub fn world_collection(&self) -> CollectionId {
        self.world_collection
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

    /// Adds an empty collection after the others. The globe does not check
    /// it. `None` if the name is blank or another collection has it.
    pub fn add_collection(&mut self, name: &str) -> Option<CollectionId> {
        if !self.collection_name_is_free(name, None) {
            return None;
        }
        let id = CollectionId(self.next_collection_id);
        self.next_collection_id += 1;
        self.collections.push(Collection {
            id,
            name: name.trim().to_owned(),
            layers: Vec::new(),
        });
        Some(id)
    }

    /// False if the collection is missing, or if the name is blank or
    /// another collection has it.
    pub fn rename_collection(&mut self, id: CollectionId, name: &str) -> bool {
        if !self.collection_name_is_free(name, Some(id)) {
            return false;
        }
        match self.collection_mut(id) {
            Some(c) => {
                c.name = name.trim().to_owned();
                true
            }
            None => false,
        }
    }

    /// Removes a collection. Each of its layers that is in no other
    /// collection goes on top of World, in the order of the collection.
    /// The globe and the maps lose the collection. False for World and for
    /// a missing collection.
    pub fn remove_collection(&mut self, id: CollectionId) -> bool {
        if id == self.world_collection {
            return false;
        }
        let Some(at) = self.collections.iter().position(|c| c.id == id) else {
            return false;
        };
        let removed = self.collections.remove(at);
        for layer in removed.layers {
            self.keep_in_a_collection(layer);
        }
        self.globe_collections.retain(|c| *c != id);
        for stack in self.map_stacks.values_mut() {
            stack.retain(|c| *c != id);
        }
        true
    }

    /// Puts a layer on top of a collection, or takes it out. A layer that
    /// leaves its last collection goes on top of World. False if the
    /// collection or the layer is missing, if nothing changes, or if the
    /// layer is only in World and `on` is false.
    pub fn set_in_collection(&mut self, id: CollectionId, layer: LayerId, on: bool) -> bool {
        if self.layer(layer).is_none() || (!on && !self.can_leave_collection(id, layer)) {
            return false;
        }
        let Some(c) = self.collection_mut(id) else {
            return false;
        };
        let has = c.layers.contains(&layer);
        match (on, has) {
            (true, false) => c.layers.push(layer),
            (false, true) => {
                c.layers.retain(|l| *l != layer);
                self.keep_in_a_collection(layer);
            }
            _ => return false,
        }
        true
    }

    /// False if the layer is only in World and `id` is World. A layer that
    /// is not in the collection can leave it.
    pub fn can_leave_collection(&self, id: CollectionId, layer: LayerId) -> bool {
        id != self.world_collection
            || self
                .collections
                .iter()
                .any(|c| c.id != id && c.layers.contains(&layer))
    }

    /// Moves a layer to index `to` of a collection. Index 0 is the bottom.
    /// False if the layer is not in the collection or nothing changes.
    pub fn move_in_collection(&mut self, id: CollectionId, layer: LayerId, to: usize) -> bool {
        let Some(c) = self.collection_mut(id) else {
            return false;
        };
        let Some(from) = c.layers.iter().position(|l| *l == layer) else {
            return false;
        };
        let to = to.min(c.layers.len() - 1);
        if from == to {
            return false;
        }
        c.layers.remove(from);
        c.layers.insert(to, layer);
        true
    }

    /// The collections that hold a layer, in the order of creation.
    pub fn collections_of(&self, layer: LayerId) -> Vec<&Collection> {
        self.collections
            .iter()
            .filter(|c| c.layers.contains(&layer))
            .collect()
    }

    /// The checked collections of the globe, from the bottom to the top.
    pub fn globe_collections(&self) -> &[CollectionId] {
        &self.globe_collections
    }

    /// Sets the checked collections of the globe and their order, from the
    /// bottom to the top. False if a collection is missing or is in the
    /// list twice.
    pub fn set_globe_collections(&mut self, ids: Vec<CollectionId>) -> bool {
        if !self.is_stack(&ids) {
            return false;
        }
        self.globe_collections = ids;
        true
    }

    /// Checks a collection on top of the globe's stack, or clears its
    /// check. False if the collection is missing or nothing changes.
    pub fn check_collection(&mut self, id: CollectionId, on: bool) -> bool {
        let checked = self.globe_collections.contains(&id);
        if self.collection(id).is_none() || checked == on {
            return false;
        }
        if on {
            self.globe_collections.push(id);
        } else {
            self.globe_collections.retain(|c| *c != id);
        }
        true
    }

    /// True if the Layers panel shows the collection as its header row
    /// alone.
    pub fn is_collapsed(&self, id: CollectionId) -> bool {
        self.collapsed_collections.contains(&id)
    }

    /// Collapses a collection in the Layers panel, or expands it. This is
    /// not an edit, so it gets no undo step. False if the collection is
    /// missing or nothing changes.
    pub fn set_collapsed(&mut self, id: CollectionId, collapsed: bool) -> bool {
        if self.collection(id).is_none() {
            return false;
        }
        match collapsed {
            true => self.collapsed_collections.insert(id),
            false => self.collapsed_collections.remove(&id),
        }
    }

    /// Moves a checked collection of the globe to index `to` of the stack.
    /// Index 0 is the bottom. False if the globe does not check the
    /// collection or nothing changes.
    pub fn move_globe_collection(&mut self, id: CollectionId, to: usize) -> bool {
        let stack = &mut self.globe_collections;
        let Some(from) = stack.iter().position(|c| *c == id) else {
            return false;
        };
        let to = to.min(stack.len() - 1);
        if from == to {
            return false;
        }
        stack.remove(from);
        stack.insert(to, id);
        true
    }

    /// True if a checked collection of the globe above `id` holds the
    /// layer, so that the layer draws at its place there.
    pub fn drawn_above(&self, id: CollectionId, layer: LayerId) -> bool {
        let stack = &self.globe_collections;
        let Some(at) = stack.iter().position(|c| *c == id) else {
            return false;
        };
        (stack[at + 1..].iter())
            .filter_map(|c| self.collection(*c))
            .any(|c| c.layers.contains(&layer))
    }

    /// The collections of a map, from the bottom to the top. A map with no
    /// stack of its own has World alone.
    pub fn map_stack(&self, map: MapId) -> Vec<CollectionId> {
        match self.map_stacks.get(&map) {
            Some(stack) => stack.clone(),
            None => vec![self.world_collection],
        }
    }

    /// Sets the collections of a map. False if the map or a collection is
    /// missing, or if a collection is in the list twice.
    pub fn set_map_stack(&mut self, map: MapId, ids: Vec<CollectionId>) -> bool {
        if self.atlas.map(map).is_none() || !self.is_stack(&ids) {
            return false;
        }
        self.map_stacks.insert(map, ids);
        true
    }

    /// True if each collection exists and is in the list one time.
    fn is_stack(&self, ids: &[CollectionId]) -> bool {
        let mut seen = BTreeSet::new();
        ids.iter()
            .all(|id| self.collection(*id).is_some() && seen.insert(*id))
    }

    /// The layers of the checked collections of the globe, in the order of
    /// the layer records.
    pub fn shown_layers(&self) -> Vec<&Layer> {
        let shown: BTreeSet<LayerId> = (self.globe_collections.iter())
            .filter_map(|id| self.collection(*id))
            .flat_map(|c| c.layers.iter().copied())
            .collect();
        self.layers
            .iter()
            .filter(|l| shown.contains(&l.id))
            .collect()
    }

    /// The collection that a new layer joins: the top checked collection
    /// of the globe, or World if the globe checks no collection.
    pub fn new_layer_collection(&self) -> CollectionId {
        self.globe_collections
            .last()
            .copied()
            .unwrap_or(self.world_collection)
    }

    /// A new layer ID. The layer goes on top of `new_layer_collection`.
    pub(crate) fn new_layer_id(&mut self) -> LayerId {
        let id = LayerId(self.next_id);
        self.next_id += 1;
        let into = self.new_layer_collection();
        if let Some(c) = self.collection_mut(into) {
            c.layers.push(id);
        }
        id
    }

    /// Takes each removed layer out of each collection.
    pub(crate) fn drop_removed_members(&mut self) {
        let ids: BTreeSet<LayerId> = self.layers.iter().map(|l| l.id).collect();
        for c in &mut self.collections {
            c.layers.retain(|id| ids.contains(id));
        }
    }

    /// Puts a layer that is in no collection on top of World.
    fn keep_in_a_collection(&mut self, layer: LayerId) {
        if self.collections.iter().any(|c| c.layers.contains(&layer)) {
            return;
        }
        let world = self.world_collection;
        if let Some(c) = self.collection_mut(world) {
            c.layers.push(layer);
        }
    }

    /// Removes the members that are not layers, a second copy of a member,
    /// and the stack entries of missing collections. Each layer that is in
    /// no collection goes on top of World, in the order of the layer
    /// records.
    pub(crate) fn prune_collections(&mut self) {
        let ids: BTreeSet<LayerId> = self.layers.iter().map(|l| l.id).collect();
        for c in &mut self.collections {
            let mut seen = BTreeSet::new();
            c.layers.retain(|id| ids.contains(id) && seen.insert(*id));
        }
        let names: BTreeSet<CollectionId> = self.collections.iter().map(|c| c.id).collect();
        let mut seen = BTreeSet::new();
        (self.globe_collections).retain(|id| names.contains(id) && seen.insert(*id));
        for stack in self.map_stacks.values_mut() {
            let mut seen = BTreeSet::new();
            stack.retain(|id| names.contains(id) && seen.insert(*id));
        }
        let layers: Vec<LayerId> = self.layers.iter().map(|l| l.id).collect();
        for layer in layers {
            self.keep_in_a_collection(layer);
        }
    }

    /// The state of the collections that an undo step keeps.
    pub(crate) fn same_collections(&self, other: &Project) -> bool {
        self.collections == other.collections
            && self.globe_collections == other.globe_collections
            && self.map_stacks == other.map_stacks
    }
}

#[cfg(test)]
mod tests {
    use super::CollectionId;
    use crate::{GeometryKind, LayerId, Project, World};

    fn project() -> (Project, [LayerId; 3]) {
        let mut p = Project::new(World::default());
        let ids = ["a", "b", "c"].map(|n| p.add_layer(n.into(), GeometryKind::Point, None, vec![]));
        (p, ids)
    }

    fn layers(p: &Project, id: CollectionId) -> Vec<LayerId> {
        p.collection(id).unwrap().layers.clone()
    }

    #[test]
    fn a_new_project_has_world_and_each_new_layer_goes_into_world() {
        let (p, ids) = project();
        let world = p.world_collection();
        assert_eq!(p.collections().len(), 1);
        assert_eq!(p.collection(world).unwrap().name, "World");
        assert_eq!(p.globe_collections(), [world]);
        assert_eq!(layers(&p, world), ids);
    }

    #[test]
    fn names_are_trimmed_and_unique() {
        let (mut p, _) = project();
        let a = p.add_collection("  Rivers ").unwrap();
        assert_eq!(p.collection(a).unwrap().name, "Rivers");
        assert_eq!(p.add_collection("Rivers"), None);
        assert_eq!(p.add_collection("World"), None);
        assert_eq!(p.add_collection("  "), None);
        let b = p.add_collection(&p.free_collection_name()).unwrap();
        assert_eq!(p.collection(b).unwrap().name, "Collection 1");
        assert!(!p.rename_collection(b, "Rivers"));
        assert!(p.rename_collection(a, "Rivers"));
        assert!(p.rename_collection(a, "Roads"));
    }

    #[test]
    fn world_can_be_renamed_and_cannot_be_removed() {
        let (mut p, ids) = project();
        let world = p.world_collection();
        assert!(p.rename_collection(world, "Earth"));
        assert_eq!(p.collection(world).unwrap().name, "Earth");
        assert!(!p.remove_collection(world));
        assert_eq!(layers(&p, world), ids);
    }

    #[test]
    fn a_layer_can_be_in_two_collections_and_out_of_world() {
        let (mut p, [a, ..]) = project();
        let world = p.world_collection();
        let c = p.add_collection("C").unwrap();
        let d = p.add_collection("D").unwrap();
        assert!(p.set_in_collection(c, a, true));
        assert!(p.set_in_collection(d, a, true));
        assert!(p.set_in_collection(world, a, false));
        let names: Vec<&str> = p
            .collections_of(a)
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["C", "D"]);
    }

    #[test]
    fn a_layer_that_leaves_its_last_collection_goes_on_top_of_world() {
        let (mut p, [a, b, c]) = project();
        let world = p.world_collection();
        let x = p.add_collection("X").unwrap();
        p.set_in_collection(x, a, true);
        p.set_in_collection(world, a, false);
        assert_eq!(layers(&p, world), [b, c]);
        assert!(p.set_in_collection(x, a, false));
        assert_eq!(layers(&p, world), [b, c, a]);
        assert_eq!(layers(&p, x), []);
    }

    #[test]
    fn a_layer_only_in_world_cannot_leave_world() {
        let (mut p, [a, ..]) = project();
        let world = p.world_collection();
        assert!(!p.can_leave_collection(world, a));
        assert!(!p.set_in_collection(world, a, false));
        assert!(p.collection(world).unwrap().layers.contains(&a));
        let x = p.add_collection("X").unwrap();
        p.set_in_collection(x, a, true);
        assert!(p.can_leave_collection(world, a));
        assert!(p.can_leave_collection(x, a));
    }

    #[test]
    fn a_removed_collection_puts_its_lone_layers_into_world() {
        let (mut p, [a, b, c]) = project();
        let world = p.world_collection();
        let x = p.add_collection("X").unwrap();
        let y = p.add_collection("Y").unwrap();
        for id in [c, a, b] {
            p.set_in_collection(x, id, true);
        }
        p.set_in_collection(y, b, true);
        for id in [a, b, c] {
            p.set_in_collection(world, id, false);
        }
        assert_eq!(layers(&p, world), []);
        p.set_globe_collections(vec![world, x, y]);
        let map = p.atlas.maps[0].id;
        p.set_map_stack(map, vec![x, world]);
        assert!(p.remove_collection(x));
        // b stays in Y alone.
        assert_eq!(layers(&p, world), [c, a]);
        assert_eq!(layers(&p, y), [b]);
        assert_eq!(p.globe_collections(), [world, y]);
        assert_eq!(p.map_stack(map), [world]);
    }

    #[test]
    fn a_new_layer_joins_the_top_checked_collection_or_world() {
        let (mut p, _) = project();
        let world = p.world_collection();
        let x = p.add_collection("X").unwrap();
        let y = p.add_collection("Y").unwrap();
        p.set_globe_collections(vec![y, x]);
        let d = p.add_layer("d".into(), GeometryKind::Line, None, vec![]);
        assert_eq!(layers(&p, x), [d]);
        assert!(!p.collection(world).unwrap().layers.contains(&d));
        p.set_globe_collections(vec![]);
        let e = p.add_layer("e".into(), GeometryKind::Line, None, vec![]);
        assert_eq!(layers(&p, world).last(), Some(&e));
    }

    #[test]
    fn layers_move_inside_a_collection() {
        let (mut p, [a, b, c]) = project();
        let world = p.world_collection();
        assert!(p.move_in_collection(world, a, 2));
        assert_eq!(layers(&p, world), [b, c, a]);
        assert!(p.move_in_collection(world, a, 0));
        assert_eq!(layers(&p, world), [a, b, c]);
        assert!(!p.move_in_collection(world, a, 0));
        let x = p.add_collection("X").unwrap();
        assert!(!p.move_in_collection(x, a, 0));
    }

    #[test]
    fn the_globe_checks_collections_in_a_stack() {
        let (mut p, [a, _, c]) = project();
        let world = p.world_collection();
        let x = p.add_collection("X").unwrap();
        p.set_in_collection(x, c, true);
        p.set_in_collection(x, a, true);
        assert!(p.check_collection(x, true));
        assert!(!p.check_collection(x, true));
        assert_eq!(p.globe_collections(), [world, x]);
        assert!(p.check_collection(world, false));
        let shown: Vec<_> = p.shown_layers().iter().map(|l| l.id).collect();
        assert_eq!(shown, [a, c]);
        assert!(!p.set_globe_collections(vec![x, x]));
        assert!(!p.set_globe_collections(vec![CollectionId(99)]));
        assert!(p.set_globe_collections(vec![x, world]));
        assert_eq!(p.shown_layers().len(), 3);
    }

    #[test]
    fn checked_collections_restack() {
        let (mut p, _) = project();
        let world = p.world_collection();
        let x = p.add_collection("X").unwrap();
        let y = p.add_collection("Y").unwrap();
        p.set_globe_collections(vec![world, x]);
        assert!(p.move_globe_collection(world, 1));
        assert_eq!(p.globe_collections(), [x, world]);
        assert!(!p.move_globe_collection(world, 5));
        assert!(!p.move_globe_collection(y, 0));
    }

    #[test]
    fn a_layer_is_drawn_above_when_a_higher_checked_collection_holds_it() {
        let (mut p, [a, b, _]) = project();
        let world = p.world_collection();
        let x = p.add_collection("X").unwrap();
        let y = p.add_collection("Y").unwrap();
        p.set_in_collection(x, a, true);
        p.set_in_collection(y, b, true);
        p.set_globe_collections(vec![world, x]);
        assert!(p.drawn_above(world, a));
        assert!(!p.drawn_above(world, b));
        assert!(!p.drawn_above(x, a));
        p.set_globe_collections(vec![x, world]);
        assert!(!p.drawn_above(world, a));
        assert!(p.drawn_above(x, a));
        assert!(!p.drawn_above(y, b));
    }

    #[test]
    fn collections_collapse() {
        let (mut p, _) = project();
        let world = p.world_collection();
        assert!(!p.is_collapsed(world));
        assert!(p.set_collapsed(world, true));
        assert!(!p.set_collapsed(world, true));
        assert!(p.is_collapsed(world));
        assert!(!p.set_collapsed(CollectionId(99), true));
        assert!(p.set_collapsed(world, false));
        assert!(!p.is_collapsed(world));
    }

    #[test]
    fn a_map_has_world_alone_until_it_gets_a_stack() {
        let (mut p, _) = project();
        let world = p.world_collection();
        let map = p.atlas.add_map();
        assert_eq!(p.map_stack(map), [world]);
        let x = p.add_collection("X").unwrap();
        assert!(p.set_map_stack(map, vec![world, x]));
        assert_eq!(p.map_stack(map), [world, x]);
        assert!(!p.set_map_stack(crate::MapId(99), vec![]));
    }

    #[test]
    fn a_removed_layer_leaves_each_collection() {
        let (mut p, [a, b, c]) = project();
        let x = p.add_collection("X").unwrap();
        p.set_in_collection(x, a, true);
        p.remove_layer(a);
        assert_eq!(layers(&p, p.world_collection()), [b, c]);
        assert_eq!(layers(&p, x), []);
    }
}
