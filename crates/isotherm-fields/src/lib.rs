//! The result grids of the climate model, on the cube map of the world.

pub use isotherm_world::World;

/// The number of texels of the six faces of a cube map.
pub fn cells(face_size: usize) -> usize {
    6 * face_size * face_size
}

/// An output of the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Output {
    /// The surface air temperature at the height of the cell, in °C.
    Temperature,
    /// The fraction of the cell under ice, from 0 to 1.
    IceCover,
}

impl Output {
    pub const ALL: [Output; 2] = [Output::Temperature, Output::IceCover];

    /// The name of the output in the world file.
    pub fn key(self) -> &'static str {
        match self {
            Output::Temperature => "temperature",
            Output::IceCover => "ice_cover",
        }
    }

    pub fn from_key(key: &str) -> Option<Output> {
        Output::ALL.into_iter().find(|o| o.key() == key)
    }
}

/// A month from 0 to 11, or the year.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Month(usize),
    Year,
}

/// One output of the model: a grid for each month and the yearly mean.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub face_size: usize,
    pub months: [Vec<f32>; 12],
    pub year: Vec<f32>,
    /// The months that hold a mean.
    pub filled: [bool; 12],
}

impl Field {
    /// A field with all values at zero and no month filled.
    pub fn new(face_size: usize) -> Field {
        let n = cells(face_size);
        Field {
            face_size,
            months: std::array::from_fn(|_| vec![0.0; n]),
            year: vec![0.0; n],
            filled: [false; 12],
        }
    }

    /// Sets the yearly mean of each cell to the mean of the twelve months.
    pub fn update_year(&mut self) {
        for (i, year) in self.year.iter_mut().enumerate() {
            let sum: f32 = self.months.iter().map(|month| month[i]).sum();
            *year = sum / 12.0;
        }
    }

    /// The grid of a period, or `None` before the model fills it. The year
    /// is filled when all twelve months are.
    pub fn get(&self, period: Period) -> Option<&[f32]> {
        match period {
            Period::Month(m) => self.filled[m].then_some(&self.months[m][..]),
            Period::Year => self.filled.iter().all(|&f| f).then_some(&self.year[..]),
        }
    }
}

/// The sums of the month that the model is in.
#[derive(Clone, Debug, PartialEq)]
pub struct Partial {
    pub month: usize,
    /// The number of steps in the sums.
    pub count: u32,
    /// One sum for each output, in the order of the outputs of [`Means`].
    pub sums: Vec<Vec<f64>>,
}

/// The monthly and yearly means of the outputs of a run. A month that ends
/// replaces the mean of the same month of the year before.
#[derive(Clone, Debug, PartialEq)]
pub struct Means {
    outputs: Vec<Output>,
    fields: Vec<Field>,
    partial: Partial,
}

impl Means {
    pub fn new(face_size: usize, outputs: &[Output]) -> Means {
        let n = cells(face_size);
        Means {
            outputs: outputs.to_vec(),
            fields: outputs.iter().map(|_| Field::new(face_size)).collect(),
            partial: Partial {
                month: 0,
                count: 0,
                sums: outputs.iter().map(|_| vec![0.0; n]).collect(),
            },
        }
    }

    /// Joins saved fields and sums. `None` if the parts do not match.
    pub fn from_parts(outputs: &[Output], fields: Vec<Field>, partial: Partial) -> Option<Means> {
        let n = fields.first().map(|f| cells(f.face_size))?;
        let fits = fields.len() == outputs.len()
            && partial.sums.len() == outputs.len()
            && partial.month < 12
            && fields.iter().all(|f| cells(f.face_size) == n)
            && partial.sums.iter().all(|s| s.len() == n);
        fits.then(|| Means {
            outputs: outputs.to_vec(),
            fields,
            partial,
        })
    }

    pub fn outputs(&self) -> &[Output] {
        &self.outputs
    }

    /// Adds the values of one step in `month`, one slice for each output.
    /// A step in another month first closes the month of the sums.
    pub fn add(&mut self, month: usize, values: &[&[f64]]) {
        assert_eq!(values.len(), self.outputs.len());
        if month != self.partial.month {
            self.close();
            self.partial.month = month;
        }
        for (sum, v) in self.partial.sums.iter_mut().zip(values) {
            for (s, x) in sum.iter_mut().zip(v.iter()) {
                *s += x;
            }
        }
        self.partial.count += 1;
    }

    /// Writes the mean of the sums to their month and clears the sums.
    pub fn close(&mut self) {
        let p = &mut self.partial;
        if p.count == 0 {
            return;
        }
        let k = f64::from(p.count);
        for (field, sum) in self.fields.iter_mut().zip(&mut p.sums) {
            for (out, s) in field.months[p.month].iter_mut().zip(sum.iter_mut()) {
                *out = (*s / k) as f32;
                *s = 0.0;
            }
            field.filled[p.month] = true;
            if field.filled.iter().all(|&f| f) {
                field.update_year();
            }
        }
        p.count = 0;
    }

    /// The field of an output. Panics if the run does not keep it.
    pub fn field(&self, output: Output) -> &Field {
        let i = self.outputs.iter().position(|&o| o == output).unwrap();
        &self.fields[i]
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn partial(&self) -> &Partial {
        &self.partial
    }

    /// The bytes that the grids allocate on the heap.
    pub fn heap_bytes(&self) -> u64 {
        let f32s: usize = self
            .fields
            .iter()
            .map(|f| f.year.capacity() + f.months.iter().map(Vec::capacity).sum::<usize>())
            .sum();
        let f64s: usize = self.partial.sums.iter().map(Vec::capacity).sum();
        let vecs = self.fields.capacity() * size_of::<Field>()
            + self.partial.sums.capacity() * size_of::<Vec<f64>>()
            + self.outputs.capacity() * size_of::<Output>();
        (f32s * 4 + f64s * 8 + vecs) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_is_the_mean_of_the_months() {
        let mut field = Field::new(2);
        assert_eq!(field.year.len(), 24);
        for (m, month) in field.months.iter_mut().enumerate() {
            month[5] = m as f32;
        }
        field.update_year();
        assert_eq!(field.year[5], 5.5);
        assert_eq!(field.year[0], 0.0);
    }

    #[test]
    fn a_month_holds_the_mean_of_its_steps_when_the_next_month_starts() {
        let mut means = Means::new(1, &[Output::Temperature]);
        means.add(0, &[&[1.0; 6]]);
        means.add(0, &[&[3.0; 6]]);
        let field = means.field(Output::Temperature);
        assert_eq!(field.get(Period::Month(0)), None);
        means.add(1, &[&[10.0; 6]]);
        let field = means.field(Output::Temperature);
        assert_eq!(field.get(Period::Month(0)), Some(&[2.0; 6][..]));
        assert_eq!(field.get(Period::Month(1)), None);
        assert_eq!(means.partial().count, 1);
    }

    #[test]
    fn the_year_fills_with_the_last_month() {
        let mut means = Means::new(1, &[Output::Temperature, Output::IceCover]);
        for m in 0..12 {
            means.add(m, &[&[m as f64; 6], &[0.5; 6]]);
        }
        assert_eq!(means.field(Output::Temperature).get(Period::Year), None);
        means.close();
        let year = means.field(Output::Temperature).get(Period::Year).unwrap();
        assert_eq!(year[0], 5.5);
        assert_eq!(
            means.field(Output::IceCover).get(Period::Year).unwrap()[3],
            0.5
        );
    }

    #[test]
    fn a_new_year_replaces_the_month() {
        let mut means = Means::new(1, &[Output::Temperature]);
        means.add(0, &[&[1.0; 6]]);
        means.add(1, &[&[1.0; 6]]);
        means.add(0, &[&[7.0; 6]]);
        means.close();
        let field = means.field(Output::Temperature);
        assert_eq!(field.get(Period::Month(0)).unwrap()[0], 7.0);
    }

    #[test]
    fn saved_parts_restore_a_partial_month() {
        let mut means = Means::new(1, &[Output::Temperature]);
        means.add(0, &[&[2.0; 6]]);
        means.add(1, &[&[4.0; 6]]);
        let restored = Means::from_parts(
            means.outputs(),
            means.fields().to_vec(),
            means.partial().clone(),
        )
        .unwrap();
        assert_eq!(restored, means);
        let bad = Partial {
            month: 0,
            count: 0,
            sums: vec![vec![0.0; 5]],
        };
        assert!(Means::from_parts(means.outputs(), means.fields().to_vec(), bad).is_none());
    }

    #[test]
    fn heap_bytes_count_the_grids() {
        let means = Means::new(2, &[Output::Temperature, Output::IceCover]);
        let grids = 2 * 24 * (13 * 4 + 8);
        let bytes = means.heap_bytes() as usize;
        assert!(bytes >= grids && bytes < grids + 1024, "{bytes}");
    }

    #[test]
    fn output_keys_round_trip() {
        for o in Output::ALL {
            assert_eq!(Output::from_key(o.key()), Some(o));
        }
    }
}
