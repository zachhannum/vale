//! The result grids of the climate model, on the cube map of the world.

pub use isotherm_world::World;

/// The number of texels of the six faces of a cube map.
pub fn cells(face_size: usize) -> usize {
    6 * face_size * face_size
}

/// One output of the model: a grid for each month and the yearly mean.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub face_size: usize,
    pub months: [Vec<f32>; 12],
    pub year: Vec<f32>,
}

impl Field {
    /// A field with all values at zero.
    pub fn new(face_size: usize) -> Field {
        let n = cells(face_size);
        Field {
            face_size,
            months: std::array::from_fn(|_| vec![0.0; n]),
            year: vec![0.0; n],
        }
    }

    /// Sets the yearly mean of each cell to the mean of the twelve months.
    pub fn update_year(&mut self) {
        for (i, year) in self.year.iter_mut().enumerate() {
            let sum: f32 = self.months.iter().map(|month| month[i]).sum();
            *year = sum / 12.0;
        }
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
}
