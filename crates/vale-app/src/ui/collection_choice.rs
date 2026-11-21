//! The collection of a new layer: the default, a collection that exists, or
//! a new collection.

use vale_store::{CollectionId, GeometryKind, LayerId};

use super::{AppState, LayerRef};

/// The choice of the Collection row of a form that makes a layer. A form
/// with no choice uses `AppState::default_layer_collection`.
#[derive(Clone, Debug, PartialEq)]
pub enum CollectionChoice {
    Existing(CollectionId),
    /// A collection with this name, made with the layer.
    New(String),
}

impl AppState {
    /// The collection of a new layer with no choice: the top collection of
    /// the map on the globe, else the top checked collection of the globe,
    /// or World if the globe checks no collection.
    pub fn default_layer_collection(&self) -> CollectionId {
        let project = &self.doc.project;
        let map = self
            .globe_map
            .and_then(|m| project.map_stack(m).last().copied());
        map.unwrap_or_else(|| project.new_layer_collection())
    }

    /// The text of the Collection row: the name of the choice only.
    pub fn collection_choice_name(&self, choice: Option<&CollectionChoice>) -> String {
        match choice {
            Some(CollectionChoice::New(name)) => name.trim().to_owned(),
            Some(CollectionChoice::Existing(id)) if self.doc.project.collection(*id).is_some() => {
                self.doc.project.collection(*id).unwrap().name.clone()
            }
            _ => {
                let id = self.default_layer_collection();
                (self.doc.project.collection(id)).map_or(String::new(), |c| c.name.clone())
            }
        }
    }

    /// True if the choice can make a layer: a new collection needs a name
    /// that is free.
    pub fn collection_choice_is_valid(&self, choice: Option<&CollectionChoice>) -> bool {
        match choice {
            Some(CollectionChoice::New(name)) => {
                self.doc.project.collection_name_is_free(name, None)
            }
            _ => true,
        }
    }

    /// Sends the next new layers to the collection of `choice`. It makes a
    /// new collection, and the globe checks the collection. Call it inside
    /// the undo step of the layer, and call `end_new_layer` after the layer.
    pub(super) fn begin_new_layer(&mut self, choice: Option<&CollectionChoice>) -> CollectionId {
        let project = &mut self.doc.project;
        let made = match choice {
            Some(CollectionChoice::New(name)) => project.add_collection(name),
            Some(CollectionChoice::Existing(id)) => project.collection(*id).map(|c| c.id),
            None => None,
        };
        let id = made.unwrap_or_else(|| self.default_layer_collection());
        let project = &mut self.doc.project;
        project.check_collection(id, true);
        project.set_layer_target(Some(id));
        id
    }

    /// Ends `begin_new_layer`.
    pub(super) fn end_new_layer(&mut self) {
        self.doc.project.set_layer_target(None);
        self.redraw_collections(true);
    }

    /// Adds an empty vector layer to the collection of `choice`, as one
    /// undo step, selects it, and opens its panel.
    pub fn add_vector_layer(
        &mut self,
        kind: GeometryKind,
        choice: Option<&CollectionChoice>,
    ) -> LayerId {
        self.save_edit();
        self.begin_new_layer(choice);
        let id = self.doc.add_layer(kind);
        self.end_new_layer();
        self.select(Some(LayerRef::Vector(id)));
        self.pad.layer = Some(LayerRef::Vector(id));
        self.touch();
        self.save_edit();
        id
    }
}
