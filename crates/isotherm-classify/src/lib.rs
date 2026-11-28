//! The climate classes of the result fields. The Köppen-Geiger thresholds
//! are those of Beck et al. (2023).

/// The main Köppen-Geiger groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MainClass {
    /// Tropical.
    A,
    /// Arid. The test needs the precipitation, so `temperature_class` never
    /// gives it.
    B,
    /// Temperate.
    C,
    /// Cold.
    D,
    /// Polar.
    E,
}

/// The lowest coldest month of group A, in °C.
pub const TROPICAL_COLD_MIN_C: f64 = 18.0;
/// The coldest month of group C is above this, in °C. Group D is at or below it.
pub const TEMPERATE_COLD_MIN_C: f64 = 0.0;
/// The warmest month of groups C and D is above this, in °C. Group E is at or below it.
pub const POLAR_HOT_MAX_C: f64 = 10.0;

/// The main group from the temperatures of the coldest month and of the
/// warmest month, in °C, for a cell that is not arid.
pub fn temperature_class(coldest_month_c: f64, warmest_month_c: f64) -> MainClass {
    if warmest_month_c <= POLAR_HOT_MAX_C {
        MainClass::E
    } else if coldest_month_c >= TROPICAL_COLD_MIN_C {
        MainClass::A
    } else if coldest_month_c > TEMPERATE_COLD_MIN_C {
        MainClass::C
    } else {
        MainClass::D
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_follow_the_thresholds() {
        assert_eq!(temperature_class(25.0, 28.0), MainClass::A);
        assert_eq!(temperature_class(18.0, 28.0), MainClass::A);
        assert_eq!(temperature_class(17.9, 28.0), MainClass::C);
        assert_eq!(temperature_class(0.1, 20.0), MainClass::C);
        assert_eq!(temperature_class(0.0, 20.0), MainClass::D);
        assert_eq!(temperature_class(-30.0, 10.1), MainClass::D);
        assert_eq!(temperature_class(-30.0, 10.0), MainClass::E);
        assert_eq!(temperature_class(-40.0, -5.0), MainClass::E);
    }
}
