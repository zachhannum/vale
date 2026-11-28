//! The sizes that each screen of the pad layout uses. All sizes are in
//! points.

/// The padding of a bar, a strip, and a segmented list.
pub const PAD: f32 = 4.0;
/// The smallest side of a control.
pub const TOUCH: f32 = 44.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidthClass {
    /// 1,000 points or more. The globe and the page fit beside the dock.
    Wide,
    /// From 600 points. The dock floats over the canvas.
    Medium,
    /// Less than 600 points. The workspace switch is a menu button, and a
    /// panel is a sheet.
    Compact,
}

impl WidthClass {
    pub fn of(width: f32) -> WidthClass {
        if width >= 1000.0 {
            WidthClass::Wide
        } else if width >= 600.0 {
            WidthClass::Medium
        } else {
            WidthClass::Compact
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_classes_change_at_1000_and_at_600_points() {
        assert_eq!(WidthClass::of(1194.0), WidthClass::Wide);
        assert_eq!(WidthClass::of(1000.0), WidthClass::Wide);
        assert_eq!(WidthClass::of(999.0), WidthClass::Medium);
        assert_eq!(WidthClass::of(600.0), WidthClass::Medium);
        assert_eq!(WidthClass::of(599.0), WidthClass::Compact);
    }
}
