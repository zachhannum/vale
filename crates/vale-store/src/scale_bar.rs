//! The scale bar of a page.

use std::ops::RangeInclusive;

use crate::atlas::{
    FrameId, FurnitureText, JsonMap, json_object, read_color, read_flag, read_number, read_string,
};
use crate::gpkg::hex;
use crate::{FillPattern, Rgb, Units};

/// An enum of choices with a name in a project file and a UI name.
macro_rules! choices {
    (
        $(#[$meta:meta])*
        $name:ident {
            $($(#[$vmeta:meta])* $variant:ident = $id:literal, $label:literal;)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            pub const ALL: [$name; <[&str]>::len(&[$($id),+])] = [$($name::$variant),+];

            /// The name in a project file.
            pub fn id(self) -> &'static str {
                match self {
                    $($name::$variant => $id,)+
                }
            }

            pub fn from_id(id: &str) -> Option<$name> {
                $name::ALL.into_iter().find(|c| c.id() == id)
            }

            /// The UI text.
            pub fn label(self) -> &'static str {
                match self {
                    $($name::$variant => $label,)+
                }
            }
        }
    };
}

choices! {
    /// The drawing of a scale bar.
    ScaleBarKind {
        /// A bar of divisions in two alternating fills.
        #[default]
        Alternating = "alternating", "Alternating";
        /// A bar of two rows of alternating fills, in a checkerboard.
        DoubleAlternating = "double_alternating", "Double alternating";
        /// A bar of outlined divisions in the second fill.
        Hollow = "hollow", "Hollow";
        /// One filled bar with no inner lines.
        SingleDivision = "single_division", "Single division";
        /// A line that steps between the top and the bottom of the bar at
        /// each division.
        SteppedLine = "stepped_line", "Stepped line";
        /// A baseline with marks.
        Line = "line", "Line";
    }
}

choices! {
    /// A unit of the distances of a scale bar.
    ScaleUnit {
        #[default]
        Kilometers = "km", "Kilometers";
        Meters = "m", "Meters";
        Miles = "mi", "Miles";
        Feet = "ft", "Feet";
        NauticalMiles = "nmi", "Nautical miles";
    }
}

choices! {
    /// What a scale bar keeps when the scale of its frame changes or its box
    /// changes size.
    ScaleFit {
        /// The bar fills the width of the box, with the largest nice
        /// division value that fits.
        #[default]
        AdjustDivisionValue = "division_value", "Adjust division value";
        /// The division value and the number of divisions stay. The bar can
        /// be wider than its box.
        AdjustWidth = "width", "Adjust width";
        /// The division value stays, and the number of divisions changes to
        /// fit the box.
        AdjustDivisions = "divisions", "Adjust divisions";
    }
}

choices! {
    /// The places on a scale bar that get a mark or a number.
    TickFrequency {
        None = "none", "None";
        Divisions = "divisions", "Divisions";
        DivisionsAndFirstMidpoint = "divisions_first_midpoint", "Divisions and first midpoint";
        #[default]
        DivisionsAndFirstSubdivisions = "divisions_first_subdivisions", "Divisions and first subdivisions";
        DivisionsAndSubdivisions = "divisions_subdivisions", "Divisions and subdivisions";
        Ends = "ends", "Ends";
        EndsAndZero = "ends_zero", "Ends and zero";
    }
}

choices! {
    /// The place of the marks or the numbers of a scale bar. The second unit
    /// of a double scale has the opposite side.
    ScalePosition {
        #[default]
        Above = "above", "Above bar";
        Below = "below", "Below bar";
        Center = "center", "Center on bar";
    }
}

choices! {
    /// The text of the unit label of a scale bar.
    UnitLabelStyle {
        /// For example "Kilometers".
        #[default]
        Name = "name", "Name";
        /// For example "km".
        Abbreviation = "abbreviation", "Abbreviation";
        /// The text of the item.
        Custom = "custom", "Custom";
    }
}

choices! {
    /// The place of the unit label of a scale bar.
    UnitPosition {
        AfterBar = "after_bar", "After bar";
        #[default]
        AfterLabels = "after_labels", "After labels";
        BeforeBar = "before_bar", "Before bar";
        BeforeLabels = "before_labels", "Before labels";
        AboveLeft = "above_left", "Above left";
        AboveCenter = "above_center", "Above center";
        BelowLeft = "below_left", "Below left";
        BelowCenter = "below_center", "Below center";
    }
}

impl ScaleUnit {
    /// Kilometers in one unit.
    pub fn km(self) -> f64 {
        match self {
            ScaleUnit::Kilometers => 1.0,
            ScaleUnit::Meters => 0.001,
            ScaleUnit::Miles => 1.609_344,
            ScaleUnit::Feet => 0.000_304_8,
            ScaleUnit::NauticalMiles => 1.852,
        }
    }

    /// The symbol, for example "km".
    pub fn abbreviation(self) -> &'static str {
        self.id()
    }

    /// The unit of a new scale bar in a world with the display units
    /// `units`.
    pub fn for_units(units: Units) -> ScaleUnit {
        match units {
            Units::Metric => ScaleUnit::Kilometers,
            Units::Imperial => ScaleUnit::Miles,
        }
    }
}

/// A scale bar of the scale of a frame. The lengths are in page millimeters,
/// and the line width is in points.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleBarItem {
    /// `None`: the scale bar has no frame.
    pub frame: Option<FrameId>,
    pub kind: ScaleBarKind,
    pub unit: ScaleUnit,
    /// A double scale: the second unit is on the other side of the baseline,
    /// with the same zero.
    pub second_unit: Option<ScaleUnit>,
    /// The divisions right of zero.
    pub divisions: u32,
    /// The parts of the first division. 0 and 1 give no parts.
    pub subdivisions: u32,
    /// One more division left of zero, with the subdivisions instead of the
    /// first division right of zero.
    pub before_zero: bool,
    /// The length of a division in the unit. `None`: the bar picks it.
    /// [`ScaleFit::AdjustDivisionValue`] always picks it.
    pub division_value: Option<f64>,
    /// The division value of the second unit.
    pub second_division_value: Option<f64>,
    pub fit: ScaleFit,
    pub bar_height: f64,
    /// The fill of the first division, and of every second one after it.
    pub fill1: FillPattern,
    pub fill2: FillPattern,
    /// The color of the outline, the lines, and the marks.
    pub outline: Rgb,
    pub line_width: f64,
    pub mark_frequency: TickFrequency,
    pub mark_position: ScalePosition,
    pub division_mark_height: f64,
    pub subdivision_mark_height: f64,
    pub number_frequency: TickFrequency,
    pub number_position: ScalePosition,
    /// The space between a number and the bar or the marks.
    pub number_gap: f64,
    /// The decimals of the numbers. `None`: each number has the fewest that
    /// show it exactly, up to 6.
    pub decimals: Option<u32>,
    /// The numbers have a separator between groups of three digits.
    pub thousands: bool,
    /// The number at zero is shown.
    pub zero_label: bool,
    pub unit_label_style: UnitLabelStyle,
    /// The unit label for [`UnitLabelStyle::Custom`].
    pub unit_label: String,
    /// The unit label of the second unit for [`UnitLabelStyle::Custom`].
    pub second_unit_label: String,
    pub unit_position: UnitPosition,
    /// The space between the unit label and the bar or the numbers.
    pub unit_gap: f64,
    pub number_style: FurnitureText,
    pub unit_style: FurnitureText,
}

impl ScaleBarItem {
    pub const DIVISIONS: RangeInclusive<u32> = 1..=20;
    pub const SUBDIVISIONS: RangeInclusive<u32> = 0..=10;
    pub const DECIMALS: RangeInclusive<u32> = 0..=6;
    /// The bar heights in millimeters that the app offers.
    pub const BAR_HEIGHTS: RangeInclusive<f64> = 0.2..=10.0;
    /// The line widths in points that the app offers.
    pub const LINE_WIDTHS: RangeInclusive<f64> = 0.1..=4.0;
    /// The mark heights in millimeters that the app offers.
    pub const MARK_HEIGHTS: RangeInclusive<f64> = 0.0..=10.0;
    /// The gaps in millimeters that the app offers.
    pub const GAPS: RangeInclusive<f64> = 0.0..=10.0;

    /// The text of the unit label of `unit`. `second` is the second unit.
    pub fn unit_text(&self, unit: ScaleUnit, second: bool) -> String {
        match self.unit_label_style {
            UnitLabelStyle::Name => unit.label().to_string(),
            UnitLabelStyle::Abbreviation => unit.abbreviation().to_string(),
            UnitLabelStyle::Custom if second => self.second_unit_label.clone(),
            UnitLabelStyle::Custom => self.unit_label.clone(),
        }
    }

    /// The item with no frame as JSON text, for a project file.
    pub fn to_json(&self) -> String {
        let mut map = JsonMap::new();
        let mut put = |key: &str, value: serde_json::Value| {
            map.insert(key.into(), value);
        };
        put("kind", self.kind.id().into());
        put("unit", self.unit.id().into());
        put("second_unit", self.second_unit.map(ScaleUnit::id).into());
        put("divisions", self.divisions.into());
        put("subdivisions", self.subdivisions.into());
        put("before_zero", self.before_zero.into());
        put("division_value", self.division_value.into());
        put("second_division_value", self.second_division_value.into());
        put("fit", self.fit.id().into());
        put("bar_height", self.bar_height.into());
        put("fill1", self.fill1.value());
        put("fill2", self.fill2.value());
        put("outline", hex(self.outline).into());
        put("line_width", self.line_width.into());
        put("mark_frequency", self.mark_frequency.id().into());
        put("mark_position", self.mark_position.id().into());
        put("division_mark_height", self.division_mark_height.into());
        put(
            "subdivision_mark_height",
            self.subdivision_mark_height.into(),
        );
        put("number_frequency", self.number_frequency.id().into());
        put("number_position", self.number_position.id().into());
        put("number_gap", self.number_gap.into());
        put("decimals", self.decimals.into());
        put("thousands", self.thousands.into());
        put("zero_label", self.zero_label.into());
        put("unit_label_style", self.unit_label_style.id().into());
        put("unit_label", self.unit_label.clone().into());
        put("second_unit_label", self.second_unit_label.clone().into());
        put("unit_position", self.unit_position.id().into());
        put("unit_gap", self.unit_gap.into());
        put("number_style", self.number_style.value());
        put("unit_style", self.unit_style.value());
        serde_json::Value::Object(map).to_string()
    }

    /// Reads [`ScaleBarItem::to_json`]. A missing key gets its default, and
    /// an unknown key is ignored. `None` if a value is bad. The keys of a
    /// text style at the top of the object apply to the numbers and the unit
    /// label.
    pub fn from_json(text: &str) -> Option<ScaleBarItem> {
        let map = json_object(text)?;
        let mut b = ScaleBarItem::default();
        fn choice<T>(
            map: &JsonMap,
            key: &str,
            to: &mut T,
            read: fn(&str) -> Option<T>,
        ) -> Option<()> {
            if let Some(v) = map.get(key) {
                *to = read(v.as_str()?)?;
            }
            Some(())
        }
        fn count(map: &JsonMap, key: &str, to: &mut u32) -> Option<()> {
            if let Some(v) = map.get(key) {
                *to = u32::try_from(v.as_u64()?).ok()?;
            }
            Some(())
        }
        fn optional<T>(
            map: &JsonMap,
            key: &str,
            to: &mut Option<T>,
            read: impl Fn(&serde_json::Value) -> Option<T>,
        ) -> Option<()> {
            match map.get(key) {
                None => {}
                Some(serde_json::Value::Null) => *to = None,
                Some(v) => *to = Some(read(v)?),
            }
            Some(())
        }
        choice(&map, "kind", &mut b.kind, ScaleBarKind::from_id)?;
        choice(&map, "unit", &mut b.unit, ScaleUnit::from_id)?;
        optional(&map, "second_unit", &mut b.second_unit, |v| {
            ScaleUnit::from_id(v.as_str()?)
        })?;
        count(&map, "divisions", &mut b.divisions)?;
        count(&map, "subdivisions", &mut b.subdivisions)?;
        read_flag(&map, "before_zero", &mut b.before_zero)?;
        let positive = |v: &serde_json::Value| v.as_f64().filter(|x| x.is_finite() && *x > 0.0);
        optional(&map, "division_value", &mut b.division_value, positive)?;
        optional(
            &map,
            "second_division_value",
            &mut b.second_division_value,
            positive,
        )?;
        choice(&map, "fit", &mut b.fit, ScaleFit::from_id)?;
        read_number(&map, "bar_height", &mut b.bar_height)?;
        if let Some(v) = map.get("fill1") {
            b.fill1 = FillPattern::read(v)?;
        }
        if let Some(v) = map.get("fill2") {
            b.fill2 = FillPattern::read(v)?;
        }
        read_color(&map, "outline", &mut b.outline)?;
        read_number(&map, "line_width", &mut b.line_width)?;
        choice(
            &map,
            "mark_frequency",
            &mut b.mark_frequency,
            TickFrequency::from_id,
        )?;
        choice(
            &map,
            "mark_position",
            &mut b.mark_position,
            ScalePosition::from_id,
        )?;
        read_number(&map, "division_mark_height", &mut b.division_mark_height)?;
        read_number(
            &map,
            "subdivision_mark_height",
            &mut b.subdivision_mark_height,
        )?;
        choice(
            &map,
            "number_frequency",
            &mut b.number_frequency,
            TickFrequency::from_id,
        )?;
        choice(
            &map,
            "number_position",
            &mut b.number_position,
            ScalePosition::from_id,
        )?;
        read_number(&map, "number_gap", &mut b.number_gap)?;
        optional(&map, "decimals", &mut b.decimals, |v| {
            u32::try_from(v.as_u64()?).ok()
        })?;
        read_flag(&map, "thousands", &mut b.thousands)?;
        read_flag(&map, "zero_label", &mut b.zero_label)?;
        choice(
            &map,
            "unit_label_style",
            &mut b.unit_label_style,
            UnitLabelStyle::from_id,
        )?;
        read_string(&map, "unit_label", &mut b.unit_label)?;
        read_string(&map, "second_unit_label", &mut b.second_unit_label)?;
        choice(
            &map,
            "unit_position",
            &mut b.unit_position,
            UnitPosition::from_id,
        )?;
        read_number(&map, "unit_gap", &mut b.unit_gap)?;
        b.number_style = (b.number_style.read(&map)?).read_key(&map, "number_style")?;
        b.unit_style = (b.unit_style.read(&map)?).read_key(&map, "unit_style")?;
        let valid = ScaleBarItem::DIVISIONS.contains(&b.divisions)
            && ScaleBarItem::SUBDIVISIONS.contains(&b.subdivisions)
            && b.decimals
                .is_none_or(|d| ScaleBarItem::DECIMALS.contains(&d))
            && b.bar_height > 0.0
            && b.line_width > 0.0
            && b.division_mark_height >= 0.0
            && b.subdivision_mark_height >= 0.0
            && b.number_gap >= 0.0
            && b.unit_gap >= 0.0;
        valid.then_some(b)
    }
}

impl Default for ScaleBarItem {
    fn default() -> Self {
        let ink = [0x2b, 0x2b, 0x2b];
        ScaleBarItem {
            frame: None,
            kind: ScaleBarKind::Alternating,
            unit: ScaleUnit::Kilometers,
            second_unit: None,
            divisions: 4,
            subdivisions: 0,
            before_zero: false,
            division_value: None,
            second_division_value: None,
            fit: ScaleFit::AdjustDivisionValue,
            bar_height: 1.5,
            fill1: FillPattern::solid(ink),
            fill2: FillPattern::solid([0xff, 0xff, 0xff]),
            outline: ink,
            line_width: 0.5,
            mark_frequency: TickFrequency::DivisionsAndFirstSubdivisions,
            mark_position: ScalePosition::Above,
            division_mark_height: 1.0,
            subdivision_mark_height: 0.6,
            number_frequency: TickFrequency::Divisions,
            number_position: ScalePosition::Above,
            number_gap: 0.6,
            decimals: None,
            thousands: true,
            zero_label: true,
            unit_label_style: UnitLabelStyle::Name,
            unit_label: String::new(),
            second_unit_label: String::new(),
            unit_position: UnitPosition::AfterLabels,
            unit_gap: 1.5,
            number_style: FurnitureText::sized(7.0),
            unit_style: FurnitureText::sized(7.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FillKind;
    use crate::atlas::TextCase;

    fn full() -> ScaleBarItem {
        ScaleBarItem {
            frame: None,
            kind: ScaleBarKind::Line,
            unit: ScaleUnit::Miles,
            second_unit: Some(ScaleUnit::Kilometers),
            divisions: 5,
            subdivisions: 2,
            before_zero: true,
            division_value: Some(30.0),
            second_division_value: Some(60.0),
            fit: ScaleFit::AdjustDivisions,
            bar_height: 2.5,
            fill1: FillPattern {
                kind: FillKind::Hatch,
                color: [1, 2, 3],
                background: Some([11, 12, 13]),
                angle: 30.0,
                spacing: 1.2,
                line_width: 0.25,
            },
            fill2: FillPattern::solid([4, 5, 6]),
            outline: [7, 8, 9],
            line_width: 0.8,
            mark_frequency: TickFrequency::EndsAndZero,
            mark_position: ScalePosition::Center,
            division_mark_height: 2.0,
            subdivision_mark_height: 1.2,
            number_frequency: TickFrequency::DivisionsAndFirstMidpoint,
            number_position: ScalePosition::Below,
            number_gap: 1.0,
            decimals: Some(2),
            thousands: false,
            zero_label: false,
            unit_label_style: UnitLabelStyle::Custom,
            unit_label: "Leagues".into(),
            second_unit_label: "Rasts".into(),
            unit_position: UnitPosition::BelowCenter,
            unit_gap: 2.0,
            number_style: FurnitureText {
                font: Some("Noto Serif".into()),
                weight: 600.0,
                italic: true,
                size: 6.5,
                color: [10, 20, 30],
                case: TextCase::AllSmallCaps,
                letter_spacing: 0.1,
                opacity: 0.5,
                halo: true,
                halo_color: [200, 210, 220],
                halo_width: 2.5,
            },
            unit_style: FurnitureText {
                case: TextCase::Upper,
                ..FurnitureText::sized(8.0)
            },
        }
    }

    #[test]
    fn a_full_scale_bar_survives_json() {
        let bar = full();
        assert_eq!(ScaleBarItem::from_json(&bar.to_json()), Some(bar));
        let none = ScaleBarItem {
            second_unit: None,
            division_value: None,
            decimals: None,
            ..full()
        };
        assert_eq!(ScaleBarItem::from_json(&none.to_json()), Some(none));
    }

    #[test]
    fn an_old_scale_bar_reads_with_defaults() {
        assert_eq!(ScaleBarItem::from_json("{}"), Some(ScaleBarItem::default()));
        // The text keys at the top are the style of the numbers and the unit.
        let old =
            r##"{"font": null, "weight": 400, "italic": false, "size": 9, "color": "102030"}"##;
        let bar = ScaleBarItem::from_json(old).unwrap();
        let style = FurnitureText {
            color: [0x10, 0x20, 0x30],
            ..FurnitureText::sized(9.0)
        };
        assert_eq!(bar.number_style, style);
        assert_eq!(bar.unit_style, style);
        assert_eq!(bar.kind, ScaleBarKind::Alternating);
        // A fill that is a color is a solid fill.
        let old = r#"{"fill1": "102030", "fill2": "ffffff"}"#;
        let bar = ScaleBarItem::from_json(old).unwrap();
        assert_eq!(bar.fill1, FillPattern::solid([0x10, 0x20, 0x30]));
        assert_eq!(bar.fill2, ScaleBarItem::default().fill2);
        for bad in [
            r#"{"divisions": 0}"#,
            r#"{"divisions": 21}"#,
            r#"{"subdivisions": 11}"#,
            r#"{"kind": "zigzag"}"#,
            r#"{"division_value": -1}"#,
            r#"{"bar_height": 0}"#,
            r#"{"number_style": {"size": 0}}"#,
            r#"{"decimals": 7}"#,
            r#"{"fill1": {"kind": "dots"}}"#,
        ] {
            assert_eq!(ScaleBarItem::from_json(bad), None, "{bad}");
        }
    }

    #[test]
    fn each_choice_has_a_name_and_a_label() {
        assert_eq!(ScaleBarKind::ALL.len(), 6);
        for k in ScaleBarKind::ALL {
            assert_eq!(ScaleBarKind::from_id(k.id()), Some(k));
        }
        assert_eq!(
            ScaleBarKind::DoubleAlternating.label(),
            "Double alternating"
        );
        assert_eq!(TickFrequency::ALL.len(), 7);
        assert_eq!(UnitPosition::ALL.len(), 8);
        assert_eq!(ScaleUnit::NauticalMiles.km(), 1.852);
        assert_eq!(ScaleUnit::for_units(Units::Imperial), ScaleUnit::Miles);
        let bar = ScaleBarItem::default();
        assert_eq!(bar.unit_text(ScaleUnit::Miles, false), "Miles");
    }
}
