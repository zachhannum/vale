//! The custom projections of the project, and the pick of a projection.

use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::CustomProjection;

use super::AppState;

/// The first word of the name of a custom projection that the app gives.
pub const CUSTOM: &str = "Custom";

/// The projection after a pick of the preset of `kind`. A new kind keeps the
/// center where it can. The kind of now goes back to the normal aspect.
pub fn pick_preset(now: ProjectionSpec, kind: ProjectionKind) -> ProjectionSpec {
    if kind == now.kind {
        ProjectionSpec {
            oblique: None,
            ..now
        }
        .normalized()
    } else {
        now.with_kind(kind)
    }
}

/// The name of the custom projection with this spec, if one has it.
pub fn custom_name(customs: &[(String, ProjectionSpec)], spec: ProjectionSpec) -> Option<&str> {
    let spec = spec.normalized();
    customs
        .iter()
        .find(|(_, s)| *s == spec)
        .map(|(name, _)| name.as_str())
}

impl AppState {
    /// The custom projections of the project, with their specs. A definition
    /// that the app cannot read is not in the list.
    pub fn custom_projections(&self) -> Vec<(String, ProjectionSpec)> {
        let list = self.doc.project.projections.iter();
        list.filter_map(|p| {
            let spec = ProjectionSpec::from_proj_string(&p.definition).ok()?;
            Some((p.name.clone(), spec))
        })
        .collect()
    }

    /// The first free name "Custom 1", "Custom 2", and so on.
    pub fn next_projection_name(&self) -> String {
        let taken = |name: &str| self.doc.project.projections.iter().any(|p| p.name == name);
        let names = (1..).map(|n| format!("{CUSTOM} {n}"));
        names
            .into_iter()
            .find(|n| !taken(n))
            .expect("a free number")
    }

    /// Adds a custom projection to the project as one undo step, and saves
    /// the project. A custom projection with the same name gets the new
    /// spec. Returns the spec that the definition gives.
    pub fn save_projection(&mut self, name: &str, spec: ProjectionSpec) -> ProjectionSpec {
        let definition = spec.definition();
        let saved = ProjectionSpec::from_proj_string(&definition).unwrap_or(spec.normalized());
        self.save_edit();
        let list = &mut self.doc.project.projections;
        match list.iter_mut().find(|p| p.name == name) {
            Some(p) => p.definition = definition,
            None => list.push(CustomProjection {
                name: name.to_owned(),
                definition,
            }),
        }
        self.save_edit();
        self.status = format!("Saved the projection {name}");
        saved
    }

    /// Removes a custom projection as one undo step, and saves the project.
    /// Returns `false` if the project has no projection with this name.
    pub fn delete_projection(&mut self, name: &str) -> bool {
        self.save_edit();
        let list = &mut self.doc.project.projections;
        let len = list.len();
        list.retain(|p| p.name != name);
        let removed = list.len() != len;
        self.save_edit();
        removed
    }
}
