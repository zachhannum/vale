//! The text size setting of the system.

/// A text size of the system, from the setting Larger Text in iOS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum TextSize {
    #[value(name = "xs")]
    ExtraSmall,
    #[value(name = "s")]
    Small,
    #[value(name = "m")]
    Medium,
    #[default]
    #[value(name = "l")]
    Large,
    #[value(name = "xl")]
    ExtraLarge,
    #[value(name = "xxl")]
    ExtraExtraLarge,
    #[value(name = "xxxl")]
    ExtraExtraExtraLarge,
    #[value(name = "ax1")]
    Accessibility1,
    #[value(name = "ax2")]
    Accessibility2,
    #[value(name = "ax3")]
    Accessibility3,
    #[value(name = "ax4")]
    Accessibility4,
    #[value(name = "ax5")]
    Accessibility5,
}

/// The largest factor. The panels have a fixed width, so the accessibility
/// sizes above it give the same text.
pub const MAX_SCALE: f32 = 28.0 / 17.0;

impl TextSize {
    /// The size of body text in points, as iOS gives it.
    fn body(self) -> f32 {
        match self {
            TextSize::ExtraSmall => 14.0,
            TextSize::Small => 15.0,
            TextSize::Medium => 16.0,
            TextSize::Large => 17.0,
            TextSize::ExtraLarge => 19.0,
            TextSize::ExtraExtraLarge => 21.0,
            TextSize::ExtraExtraExtraLarge => 23.0,
            TextSize::Accessibility1 => 28.0,
            TextSize::Accessibility2 => 33.0,
            TextSize::Accessibility3 => 40.0,
            TextSize::Accessibility4 => 47.0,
            TextSize::Accessibility5 => 53.0,
        }
    }

    /// The factor on the text sizes of the UI. Large gives 1.
    pub fn scale(self) -> f32 {
        (self.body() / TextSize::Large.body()).min(MAX_SCALE)
    }

    /// The text size that the system has now.
    #[cfg(target_os = "ios")]
    pub fn system() -> TextSize {
        use objc2_ui_kit::*;
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return TextSize::Large;
        };
        let category = UIApplication::sharedApplication(mtm).preferredContentSizeCategory();
        // SAFETY: the statics are constant strings of UIKit.
        let table = unsafe {
            [
                (UIContentSizeCategoryExtraSmall, TextSize::ExtraSmall),
                (UIContentSizeCategorySmall, TextSize::Small),
                (UIContentSizeCategoryMedium, TextSize::Medium),
                (UIContentSizeCategoryExtraLarge, TextSize::ExtraLarge),
                (
                    UIContentSizeCategoryExtraExtraLarge,
                    TextSize::ExtraExtraLarge,
                ),
                (
                    UIContentSizeCategoryExtraExtraExtraLarge,
                    TextSize::ExtraExtraExtraLarge,
                ),
                (
                    UIContentSizeCategoryAccessibilityMedium,
                    TextSize::Accessibility1,
                ),
                (
                    UIContentSizeCategoryAccessibilityLarge,
                    TextSize::Accessibility2,
                ),
                (
                    UIContentSizeCategoryAccessibilityExtraLarge,
                    TextSize::Accessibility3,
                ),
                (
                    UIContentSizeCategoryAccessibilityExtraExtraLarge,
                    TextSize::Accessibility4,
                ),
                (
                    UIContentSizeCategoryAccessibilityExtraExtraExtraLarge,
                    TextSize::Accessibility5,
                ),
            ]
        };
        table
            .into_iter()
            .find(|(c, _)| **c == *category)
            .map_or(TextSize::Large, |(_, size)| size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_is_the_size_of_the_design() {
        assert_eq!(TextSize::default().scale(), 1.0);
    }

    #[test]
    fn the_factor_grows_with_the_size_up_to_the_limit() {
        let sizes = [
            TextSize::ExtraSmall,
            TextSize::Small,
            TextSize::Medium,
            TextSize::Large,
            TextSize::ExtraLarge,
            TextSize::ExtraExtraLarge,
            TextSize::ExtraExtraExtraLarge,
            TextSize::Accessibility1,
        ];
        for pair in sizes.windows(2) {
            assert!(pair[0].scale() < pair[1].scale(), "{pair:?}");
        }
        assert_eq!(TextSize::Accessibility1.scale(), MAX_SCALE);
        assert_eq!(TextSize::Accessibility5.scale(), MAX_SCALE);
    }
}
