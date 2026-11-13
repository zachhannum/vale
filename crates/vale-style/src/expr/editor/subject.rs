//! What a filter rule compares or a label text field shows.

use vale_store::{FieldType, GeometryKind, Value};

use super::super::field_ref;
use super::super::parser::{Node, Var};
use super::super::print::quote;
use super::super::{Measure, Stat, Unit};

/// A field, a variable, a measure of the geometry, or the value of a raster layer.
#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    Field(String),
    Var(Var),
    /// A measure and its unit. `None` writes the call with no unit, which has the default unit.
    Measure(Measure, Option<Unit>),
    /// The name of a raster layer and the stat. `None` writes the call with no stat, which has the default stat.
    Raster(String, Option<Stat>),
}

impl Subject {
    pub fn to_text(&self) -> String {
        match self {
            Subject::Field(name) => field_ref(name),
            Subject::Var(v) => format!("@{}", v.name()),
            Subject::Measure(m, None) => format!("{}()", m.name()),
            Subject::Measure(m, Some(u)) => format!("{}({})", m.name(), quote(u.code())),
            Subject::Raster(layer, None) => format!("raster({})", quote(layer)),
            Subject::Raster(layer, Some(st)) => {
                format!("raster({}, {})", quote(layer), quote(st.code()))
            }
        }
    }

    /// The field name, the noun of a variable or a measure, or the name of a raster layer with its stat, for example "Height (max)".
    pub fn label(&self) -> String {
        match self {
            Subject::Field(name) => name.clone(),
            Subject::Var(v) => v.label().to_owned(),
            Subject::Measure(m, _) => m.label().to_owned(),
            Subject::Raster(layer, None) => layer.clone(),
            Subject::Raster(layer, Some(st)) => format!("{layer} ({})", st.code()),
        }
    }

    pub fn field(&self) -> Option<&str> {
        match self {
            Subject::Field(name) => Some(name),
            _ => None,
        }
    }

    /// The type of the value. `of_field` gives the type of a field.
    pub fn field_type(
        &self,
        of_field: impl FnOnce(&str) -> Option<FieldType>,
    ) -> Option<FieldType> {
        match self {
            Subject::Field(name) => of_field(name),
            Subject::Var(v) => Some(v.field_type()),
            Subject::Measure(..) | Subject::Raster(..) => Some(FieldType::Real),
        }
    }

    /// True for the same field, the same variable, the same measure with any unit, or the same raster layer with any stat.
    pub fn same_kind(&self, other: &Subject) -> bool {
        match (self, other) {
            (Subject::Measure(a, _), Subject::Measure(b, _)) => a == b,
            (Subject::Raster(a, _), Subject::Raster(b, _)) => a == b,
            _ => self == other,
        }
    }
}

/// The subject of a node. A measure reads only with no argument or with the exact code of one of its units. A raster call reads only with a text name and no stat or the exact code of a stat.
pub(super) fn subject_of(node: &Node) -> Option<Subject> {
    match node {
        Node::Field(name) => Some(Subject::Field(name.clone())),
        Node::Var(v) => Some(Subject::Var(*v)),
        Node::Call(f, args) if f.name == "raster" => match args.as_slice() {
            [Node::Lit(Value::Text(layer))] => Some(Subject::Raster(layer.clone(), None)),
            [Node::Lit(Value::Text(layer)), Node::Lit(Value::Text(s))] => {
                let st = Stat::ALL.into_iter().find(|st| st.code() == s)?;
                Some(Subject::Raster(layer.clone(), Some(st)))
            }
            _ => None,
        },
        Node::Call(f, args) => {
            let m = Measure::of_name(f.name)?;
            match args.as_slice() {
                [] => Some(Subject::Measure(m, None)),
                [Node::Lit(Value::Text(s))] => {
                    let u = m.units().iter().find(|u| u.code() == s)?;
                    Some(Subject::Measure(m, Some(*u)))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The measures and the variables of the geometry of a layer kind, in menu order. Each measure has its default unit.
pub fn measures_for(kind: GeometryKind) -> Vec<Subject> {
    let measures = (Measure::ALL.into_iter())
        .filter(|m| m.geometry() == kind)
        .map(|m| Subject::Measure(m, Some(m.default_unit())));
    let vars = ([Var::Vertices, Var::Parts, Var::Lat, Var::Lon].into_iter())
        .filter(|v| v.fits(kind))
        .map(Subject::Var);
    measures.chain(vars).collect()
}

/// The value subjects of the raster layers `names`, in their order. Each has no stat. Each geometry kind has them.
pub fn rasters_for(_kind: GeometryKind, names: &[String]) -> Vec<Subject> {
    (names.iter())
        .map(|name| Subject::Raster(name.clone(), None))
        .collect()
}
