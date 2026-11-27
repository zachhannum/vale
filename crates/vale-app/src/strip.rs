//! The sliders of the brush that the tool column shows: the strip sliders.
//! Each brush mode has a default pair, and you pin the sliders of the Brush
//! panel to the strip.

use vale_terrain::Mode;

/// A slider of the Brush panel. One slider can set a different setting in
/// each mode: `Strength` is the distance of the Warp mode and the height of
/// the Range mode, for example.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slider {
    Size,
    Hardness,
    Flow,
    Strength,
    Scale,
    Rate,
    Opacity,
    Value,
    Raise,
    Lower,
    Grain,
    Relief,
    Step,
    Shape,
    Spacing,
    Density,
    Age,
    Lakes,
    Erosion,
    Branching,
    RainShadow,
    Magma,
    Hills,
}

/// The sliders, with their names in the file of the tools.
pub const NAMES: [(Slider, &str); 23] = [
    (Slider::Size, "size"),
    (Slider::Hardness, "hardness"),
    (Slider::Flow, "flow"),
    (Slider::Strength, "strength"),
    (Slider::Scale, "scale"),
    (Slider::Rate, "rate"),
    (Slider::Opacity, "opacity"),
    (Slider::Value, "value"),
    (Slider::Raise, "raise"),
    (Slider::Lower, "lower"),
    (Slider::Grain, "grain"),
    (Slider::Relief, "relief"),
    (Slider::Step, "step"),
    (Slider::Shape, "shape"),
    (Slider::Spacing, "spacing"),
    (Slider::Density, "density"),
    (Slider::Age, "age"),
    (Slider::Lakes, "lakes"),
    (Slider::Erosion, "erosion"),
    (Slider::Branching, "branching"),
    (Slider::RainShadow, "rain_shadow"),
    (Slider::Magma, "magma"),
    (Slider::Hills, "hills"),
];

/// The most sliders that the strip shows.
pub const MOST: usize = 2;

/// The modes without the strength: these modes move the ground toward a
/// target that the strength does not set.
fn has_strength(mode: Mode) -> bool {
    !matches!(
        mode,
        Mode::Smooth
            | Mode::Deposit
            | Mode::Weather
            | Mode::Flatten
            | Mode::Paint
            | Mode::Push
            | Mode::Noise
            | Mode::Terrace
            | Mode::Shape
            | Mode::OceanRidge
            | Mode::AbyssalHills
            | Mode::Margin
    )
}

/// The sliders of the Brush panel for `mode`, in the order of the panel.
/// `height`: the layer holds heights, and the Noise mode has its relief.
pub fn sliders(mode: Mode, height: bool) -> Vec<Slider> {
    use Slider::*;
    let mut out = Vec::new();
    // The outline sets the size of a basin, and a basin has no flow.
    if mode != Mode::Basin {
        out.push(Size);
    }
    if !matches!(
        mode,
        Mode::OceanRidge | Mode::Range | Mode::Basin | Mode::Hotspot
    ) {
        out.push(Hardness);
    }
    if mode != Mode::Basin {
        out.push(Flow);
    }
    match mode {
        Mode::OceanRidge => out.extend([Magma, Hills, Scale]),
        Mode::Warp => out.extend([Strength, Scale]),
        Mode::Flatten => out.push(Rate),
        Mode::AbyssalHills => out.extend([Rate, Hills, Scale]),
        Mode::Paint => out.extend([Opacity, Value]),
        Mode::Noise => out.extend([Raise, Lower, Scale, Grain]),
        Mode::Terrace => out.push(Step),
        Mode::Shape => out.push(Shape),
        mode if has_strength(mode) => out.push(Strength),
        _ => {}
    }
    match mode {
        Mode::Noise if height => out.push(Relief),
        Mode::Range => out.push(Spacing),
        Mode::Basin => out.extend([Density, Age, Lakes]),
        Mode::Seamounts => out.extend([Density, Age]),
        _ => {}
    }
    if matches!(
        mode,
        Mode::Uplift | Mode::Range | Mode::Basin | Mode::Margin | Mode::Hotspot
    ) {
        out.push(Erosion);
    }
    if matches!(
        mode,
        Mode::Erode | Mode::Uplift | Mode::Margin | Mode::Hotspot
    ) {
        out.push(Branching);
    }
    if matches!(mode, Mode::Erode | Mode::Uplift | Mode::Range | Mode::Basin) {
        out.push(RainShadow);
    }
    out
}

/// The strip sliders of a mode that has no pins, in the order of the pins.
pub fn default_pins(mode: Mode) -> [Slider; 2] {
    use Slider::*;
    let second = match mode {
        Mode::Basin => return [Strength, Density],
        Mode::Smooth | Mode::Push | Mode::Deposit | Mode::Weather | Mode::Margin => Flow,
        Mode::Flatten => Rate,
        Mode::Paint => Value,
        Mode::Warp | Mode::Noise => Scale,
        Mode::Terrace => Step,
        Mode::Shape => Shape,
        Mode::OceanRidge => Magma,
        Mode::Seamounts => Density,
        Mode::AbyssalHills => Hills,
        _ => Strength,
    };
    [Size, second]
}

/// The pins of each brush mode that you changed. A mode that is not here
/// has the default pins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pins {
    /// The pins of each mode, in the order that you pinned them.
    modes: Vec<(Mode, Vec<Slider>)>,
}

impl Pins {
    /// The pins of `mode`, in the order that you pinned them.
    pub fn of(&self, mode: Mode) -> Vec<Slider> {
        match self.modes.iter().find(|(m, _)| *m == mode) {
            Some((_, pins)) => pins.clone(),
            None => default_pins(mode).to_vec(),
        }
    }

    /// The modes with their own pins, in the order that they got them.
    pub fn changed(&self) -> impl Iterator<Item = (Mode, &[Slider])> {
        self.modes.iter().map(|(mode, pins)| (*mode, &pins[..]))
    }

    /// Sets the pins of `mode`. The pins after the first `MOST` do not
    /// count, and neither does a second pin of one slider.
    pub fn set(&mut self, mode: Mode, pins: &[Slider]) {
        let mut kept: Vec<Slider> = Vec::new();
        for &pin in pins {
            if !kept.contains(&pin) && kept.len() < MOST {
                kept.push(pin);
            }
        }
        match self.modes.iter_mut().find(|(m, _)| *m == mode) {
            Some(slot) => slot.1 = kept,
            None => self.modes.push((mode, kept)),
        }
    }

    /// The pins of `mode` are not its default pins. The order of the pins
    /// does not count.
    pub fn is_changed(&self, mode: Mode) -> bool {
        let pins = self.of(mode);
        let default = default_pins(mode);
        pins.len() != default.len() || default.iter().any(|d| !pins.contains(d))
    }

    /// Gives `mode` its default pins.
    pub fn reset(&mut self, mode: Mode) {
        self.modes.retain(|(m, _)| *m != mode);
    }

    pub fn is_pinned(&self, mode: Mode, slider: Slider) -> bool {
        self.of(mode).contains(&slider)
    }

    /// Pins `slider` to the strip of `mode`, or unpins it. A pin with a
    /// full strip replaces the slider that you pinned last.
    pub fn toggle(&mut self, mode: Mode, slider: Slider) {
        let mut pins = self.of(mode);
        if let Some(at) = pins.iter().position(|&s| s == slider) {
            pins.remove(at);
        } else {
            if pins.len() >= MOST {
                pins.pop();
            }
            pins.push(slider);
        }
        self.set(mode, &pins);
    }

    /// The strip sliders of `mode`, in the order of the Brush panel.
    /// `height` is as for `sliders`.
    pub fn strip(&self, mode: Mode, height: bool) -> Vec<Slider> {
        let pins = self.of(mode);
        let all = sliders(mode, height);
        all.into_iter().filter(|s| pins.contains(s)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pad::{SCULPT_MODES, TERRAIN_MODES};

    fn modes() -> impl Iterator<Item = Mode> {
        SCULPT_MODES.iter().chain(&TERRAIN_MODES).map(|m| m.2)
    }

    #[test]
    fn each_mode_shows_its_default_pair() {
        use Slider::*;
        let pins = Pins::default();
        for (mode, pair) in [
            (Mode::Raise, [Size, Strength]),
            (Mode::Lower, [Size, Strength]),
            (Mode::Smooth, [Size, Flow]),
            (Mode::Flatten, [Size, Rate]),
            (Mode::Paint, [Size, Value]),
            (Mode::Warp, [Size, Scale]),
            (Mode::Push, [Size, Flow]),
            (Mode::Noise, [Size, Scale]),
            (Mode::Carve, [Size, Strength]),
            (Mode::Ridge, [Size, Strength]),
            (Mode::Deposit, [Size, Flow]),
            (Mode::Terrace, [Size, Step]),
            (Mode::Shape, [Size, Shape]),
            (Mode::Erode, [Size, Strength]),
            (Mode::Weather, [Size, Flow]),
            (Mode::Uplift, [Size, Strength]),
            (Mode::OceanRidge, [Size, Magma]),
            (Mode::Range, [Size, Strength]),
            (Mode::Basin, [Strength, Density]),
            (Mode::Seamounts, [Size, Density]),
            (Mode::AbyssalHills, [Size, Hills]),
            (Mode::Margin, [Size, Flow]),
            (Mode::Hotspot, [Size, Strength]),
        ] {
            assert_eq!(pins.strip(mode, true), pair, "{mode:?}");
        }
    }

    #[test]
    fn each_default_pin_is_a_slider_of_its_mode() {
        for mode in modes() {
            for height in [true, false] {
                let all = sliders(mode, height);
                for pin in default_pins(mode) {
                    assert!(all.contains(&pin), "{mode:?} {pin:?}");
                }
            }
        }
    }

    #[test]
    fn the_modes_that_do_not_use_the_strength_have_no_strength_slider() {
        for mode in [Mode::Smooth, Mode::Deposit, Mode::Weather] {
            assert!(!sliders(mode, true).contains(&Slider::Strength));
        }
    }

    #[test]
    fn a_third_pin_replaces_the_last_pin_and_the_strip_keeps_the_panel_order() {
        use Slider::*;
        let mut pins = Pins::default();
        pins.toggle(Mode::Raise, Hardness);
        assert_eq!(pins.strip(Mode::Raise, true), [Size, Hardness]);
        pins.toggle(Mode::Raise, Flow);
        assert_eq!(pins.strip(Mode::Raise, true), [Size, Flow]);
        // An unpin leaves room, and the next pin fills it.
        pins.toggle(Mode::Raise, Size);
        assert_eq!(pins.strip(Mode::Raise, true), [Flow]);
        pins.toggle(Mode::Raise, Strength);
        assert_eq!(pins.of(Mode::Raise), [Flow, Strength]);
        pins.toggle(Mode::Raise, Hardness);
        assert_eq!(pins.strip(Mode::Raise, true), [Hardness, Flow]);
        // The other modes keep their pins.
        assert_eq!(pins.strip(Mode::Lower, true), [Size, Strength]);
        pins.toggle(Mode::Raise, Hardness);
        pins.toggle(Mode::Raise, Flow);
        assert!(pins.strip(Mode::Raise, true).is_empty());
    }

    #[test]
    fn a_reset_gives_a_mode_its_default_pins() {
        let mut pins = Pins::default();
        assert!(!pins.is_changed(Mode::Raise));
        pins.toggle(Mode::Raise, Slider::Hardness);
        pins.toggle(Mode::Lower, Slider::Flow);
        assert!(pins.is_changed(Mode::Raise));
        pins.reset(Mode::Raise);
        assert!(!pins.is_changed(Mode::Raise));
        assert_eq!(pins.of(Mode::Raise), default_pins(Mode::Raise));
        assert!(pins.is_changed(Mode::Lower));
        // The default sliders in another order are not a change.
        pins.set(Mode::Lower, &[Slider::Strength, Slider::Size]);
        assert!(!pins.is_changed(Mode::Lower));
    }

    #[test]
    fn a_pin_that_the_layer_does_not_have_is_not_in_the_strip() {
        let mut pins = Pins::default();
        pins.toggle(Mode::Noise, Slider::Relief);
        assert_eq!(
            pins.strip(Mode::Noise, true),
            [Slider::Size, Slider::Relief]
        );
        assert_eq!(pins.strip(Mode::Noise, false), [Slider::Size]);
    }
}
