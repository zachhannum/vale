//! The value of a raster layer under a feature.

use vale_store::{Geometry, Value};

use super::Context;
use super::functions::finite;

/// How the `raster` function reduces the cells under a line or a polygon to one value.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stat {
    #[default]
    Mean,
    Min,
    Max,
}

impl Stat {
    pub const ALL: [Stat; 3] = [Stat::Mean, Stat::Min, Stat::Max];

    /// The stat argument of the `raster` function.
    pub fn code(self) -> &'static str {
        match self {
            Stat::Mean => "mean",
            Stat::Min => "min",
            Stat::Max => "max",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stat::Mean => "Mean",
            Stat::Min => "Min",
            Stat::Max => "Max",
        }
    }

    /// The stat of a code, with any case.
    pub fn parse(s: &str) -> Option<Stat> {
        (Stat::ALL.into_iter()).find(|st| st.code().eq_ignore_ascii_case(s))
    }
}

/// The values of the raster layers under the features.
pub trait RasterValues {
    /// The value of the raster layer `layer` under the feature at `index` in its layer, with the geometry `shape`. `None` if no raster layer has the name, or if the value is not known yet. A point ignores `stat`.
    fn value(&self, layer: &str, stat: Stat, index: usize, shape: &Geometry) -> Option<f64>;

    /// True if a raster layer has the name.
    fn has_layer(&self, layer: &str) -> bool;
}

pub(super) fn raster(args: &[Value], ctx: &Context) -> Value {
    let Some(Value::Text(layer)) = args.first() else {
        return Value::Null;
    };
    let stat = match args.get(1) {
        None => Stat::default(),
        Some(Value::Text(s)) => match Stat::parse(s) {
            Some(st) => st,
            None => return Value::Null,
        },
        Some(_) => return Value::Null,
    };
    let (Some(rasters), Some(shape)) = (ctx.rasters, ctx.shape) else {
        return Value::Null;
    };
    let stat = match shape {
        Geometry::Points(_) => Stat::default(),
        _ => stat,
    };
    (rasters.value(layer, stat, ctx.index, shape)).map_or(Value::Null, finite)
}
