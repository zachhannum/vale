//! The rows of the attribute table: the features of a layer, filtered by an
//! expression and sorted by a field. The rows are feature indices, and they
//! are built again only when their key changes.

use std::cmp::Ordering;
use std::collections::HashSet;

use vale_store::{Layer, LayerId, Uuid, Value};
use vale_style::expr::{Context, Filter};

/// The sort of the table: a field name, and the direction.
#[derive(Clone, Debug, PartialEq)]
pub struct Sort {
    pub field: String,
    pub ascending: bool,
}

/// All that the rows depend on. `revision` changes with each edit of the
/// features, and `selected` is the selection when the table shows only the
/// selected rows.
#[derive(Clone, Debug, PartialEq)]
pub struct RowKey {
    pub layer: LayerId,
    pub revision: (u64, u64),
    /// The radius of the world in kilometers, for the measures of a filter.
    pub radius_km: f64,
    pub len: usize,
    pub sort: Option<Sort>,
    pub filter: String,
    pub selected: Option<Vec<Uuid>>,
}

#[derive(Default)]
pub struct Rows {
    key: Option<RowKey>,
    rows: Vec<usize>,
    error: Option<String>,
    rebuilds: u64,
}

impl Rows {
    /// The rows for the key. They are built again if the key changed.
    pub fn update(&mut self, key: RowKey, layer: &Layer) -> &[usize] {
        if self.key.as_ref() != Some(&key) {
            let filter = Filter::new(&key.filter);
            self.error = filter.error().map(|e| e.to_string());
            let selected = key
                .selected
                .as_ref()
                .map(|s| s.iter().copied().collect::<HashSet<_>>());
            self.rows = build(
                layer,
                key.radius_km,
                key.sort.as_ref(),
                &filter,
                selected.as_ref(),
            );
            self.key = Some(key);
            self.rebuilds += 1;
        }
        &self.rows
    }

    pub fn rows(&self) -> &[usize] {
        &self.rows
    }

    /// The message of a wrong filter.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The number of times that the rows were built.
    pub fn rebuilds(&self) -> u64 {
        self.rebuilds
    }
}

/// The indices of the features that match the filter and, if there is a
/// set, are in the set, in the order of the sort. A wrong filter matches
/// no feature. The scale of the filter is 0, and the world has radius
/// `radius_km`.
pub fn build(
    layer: &Layer,
    radius_km: f64,
    sort: Option<&Sort>,
    filter: &Filter,
    selected: Option<&HashSet<Uuid>>,
) -> Vec<usize> {
    let mut rows: Vec<usize> = layer
        .features
        .iter()
        .enumerate()
        .filter(|(_, f)| selected.is_none_or(|s| s.contains(&f.uuid())))
        .filter(|(_, f)| {
            filter.matches(&Context {
                attributes: &f.attributes,
                scale: 0.0,
                geometry: f.geometry.kind(),
                shape: Some(&f.geometry),
                radius_km,
                page: None,
            })
        })
        .map(|(i, _)| i)
        .collect();
    if let Some(sort) = sort {
        let keys: Vec<Key> = layer
            .features
            .iter()
            .map(|f| Key::of(f.attributes.get(&sort.field)))
            .collect();
        rows.sort_by(|&a, &b| compare(&keys[a], &keys[b], sort.ascending));
    }
    rows
}

/// A value in a form that compares fast. Text compares without case.
enum Key {
    Bool(bool),
    Number(f64),
    Text(String),
    Null,
}

impl Key {
    fn of(value: Option<&Value>) -> Key {
        match value {
            Some(Value::Bool(b)) => Key::Bool(*b),
            Some(Value::Number(n)) => Key::Number(*n),
            Some(Value::Text(t)) => Key::Text(t.to_lowercase()),
            Some(Value::Null) | None => Key::Null,
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Key::Bool(_) => 0,
            Key::Number(_) => 1,
            Key::Text(_) => 2,
            Key::Null => 3,
        }
    }
}

/// Null is last in both directions.
fn compare(a: &Key, b: &Key, ascending: bool) -> Ordering {
    let order = match (a, b) {
        (Key::Null, Key::Null) => return Ordering::Equal,
        (Key::Null, _) => return Ordering::Greater,
        (_, Key::Null) => return Ordering::Less,
        (Key::Bool(a), Key::Bool(b)) => a.cmp(b),
        (Key::Number(a), Key::Number(b)) => a.total_cmp(b),
        (Key::Text(a), Key::Text(b)) => a.cmp(b),
        _ => a.rank().cmp(&b.rank()),
    };
    if ascending { order } else { order.reverse() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vale_store::{Feature, Field, FieldType, Geometry, GeometryKind, Project, World};

    const R: f64 = 6371.0;

    fn layer() -> (Project, LayerId) {
        let mut p = Project::new(World::default());
        let id = p.add_layer("Places".into(), GeometryKind::Point, None, vec![]);
        p.add_field(id, Field::new("name", FieldType::Text));
        p.add_field(id, Field::new("rank", FieldType::Integer));
        let rows = [
            ("hartle", Value::Number(3.0)),
            ("Ardent", Value::Number(1.0)),
            ("castellane", Value::Null),
            ("Zell", Value::Number(2.0)),
        ];
        for (name, rank) in rows {
            let attributes = BTreeMap::from([
                ("name".to_string(), Value::Text(name.into())),
                ("rank".to_string(), rank),
            ]);
            p.add_feature(
                id,
                Feature::new(Geometry::Points(vec![[0.0, 0.0]]), attributes),
            );
        }
        (p, id)
    }

    fn names(layer: &Layer, rows: &[usize]) -> Vec<String> {
        rows.iter()
            .map(|&i| match &layer.features[i].attributes["name"] {
                Value::Text(t) => t.clone(),
                v => panic!("{v:?}"),
            })
            .collect()
    }

    fn sort(field: &str, ascending: bool) -> Option<Sort> {
        Some(Sort {
            field: field.into(),
            ascending,
        })
    }

    #[test]
    fn text_sorts_without_case_and_null_is_last() {
        let (p, id) = layer();
        let l = p.layer(id).unwrap();
        let all = Filter::all();
        let up = build(l, R, sort("name", true).as_ref(), &all, None);
        assert_eq!(names(l, &up), ["Ardent", "castellane", "hartle", "Zell"]);
        let down = build(l, R, sort("rank", false).as_ref(), &all, None);
        assert_eq!(names(l, &down), ["hartle", "Zell", "Ardent", "castellane"]);
        let up = build(l, R, sort("rank", true).as_ref(), &all, None);
        assert_eq!(names(l, &up), ["Ardent", "Zell", "hartle", "castellane"]);
        assert_eq!(build(l, R, None, &all, None), [0, 1, 2, 3]);
    }

    #[test]
    fn the_filter_and_the_selection_both_limit_the_rows() {
        let (p, id) = layer();
        let l = p.layer(id).unwrap();
        let filter = Filter::new("rank >= 2");
        assert_eq!(build(l, R, None, &filter, None), [0, 3]);
        let selected = HashSet::from([l.features[0].uuid(), l.features[1].uuid()]);
        assert_eq!(build(l, R, None, &Filter::all(), Some(&selected)), [0, 1]);
        assert_eq!(build(l, R, None, &filter, Some(&selected)), [0]);
        assert!(build(l, R, None, &Filter::new("rank >="), None).is_empty());
    }

    #[test]
    fn the_rows_are_built_again_only_when_the_key_changes() {
        let (p, id) = layer();
        let l = p.layer(id).unwrap();
        let key = RowKey {
            layer: id,
            revision: (0, 0),
            radius_km: R,
            len: l.features.len(),
            sort: None,
            filter: String::new(),
            selected: None,
        };
        let mut rows = Rows::default();
        rows.update(key.clone(), l);
        rows.update(key.clone(), l);
        assert_eq!(rows.rebuilds(), 1);
        let wrong = RowKey {
            filter: "rank >=".into(),
            ..key
        };
        assert!(rows.update(wrong, l).is_empty());
        assert_eq!(rows.rebuilds(), 2);
        assert!(rows.error().is_some());
    }

    #[test]
    fn fifty_thousand_rows_sort_and_filter_fast() {
        let mut p = Project::new(World::default());
        let id = p.add_layer("Many".into(), GeometryKind::Point, None, vec![]);
        p.add_field(id, Field::new("name", FieldType::Text));
        p.add_field(id, Field::new("rank", FieldType::Integer));
        for i in 0..50_000u32 {
            let attributes = BTreeMap::from([
                (
                    "name".to_string(),
                    Value::Text(format!("Town {}", i.wrapping_mul(7919) % 50_000)),
                ),
                ("rank".to_string(), Value::Number(f64::from(i % 9))),
            ]);
            p.add_feature(
                id,
                Feature::new(Geometry::Points(vec![[0.0, 0.0]]), attributes),
            );
        }
        let l = p.layer(id).unwrap();
        let start = std::time::Instant::now();
        let rows = build(
            l,
            R,
            sort("name", true).as_ref(),
            &Filter::new("rank > 2"),
            None,
        );
        let time = start.elapsed();
        assert_eq!(rows.len(), (0..50_000).filter(|i| i % 9 > 2).count());
        // A debug build is many times slower than a release build.
        let limit = if cfg!(debug_assertions) { 2.0 } else { 0.1 };
        assert!(time.as_secs_f64() < limit, "{time:?}");
    }
}
