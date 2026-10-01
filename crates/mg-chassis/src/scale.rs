//! Whole-browser display size. The host supplies desktop scale; layout and input
//! remain logical while Sparkle rasterizes both pages and controls at this scale.

pub const SCALE_STEPS: [u16; 8] = [75, 100, 125, 150, 175, 200, 250, 300];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScalePreference {
    #[default]
    System,
    Percent(u16),
}

impl ScalePreference {
    /// Whether an explicit preference is one of the supported display sizes.
    pub fn is_valid(self) -> bool {
        match self {
            Self::System => true,
            Self::Percent(percent) => SCALE_STEPS.contains(&percent),
        }
    }

    pub(super) fn validated(self) -> Self {
        if self.is_valid() { self } else { Self::System }
    }

    pub(super) fn resolve(self, system: u16) -> u16 {
        match self.validated() {
            Self::System => system.clamp(75, 300),
            Self::Percent(percent) => percent,
        }
    }
}

pub(super) fn next_step(percent: u16, increase: bool) -> u16 {
    if increase {
        SCALE_STEPS
            .into_iter()
            .find(|step| *step > percent)
            .unwrap_or(300)
    } else {
        SCALE_STEPS
            .into_iter()
            .rev()
            .find(|step| *step < percent)
            .unwrap_or(75)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_preferences_validate_and_system_is_bounded() {
        assert_eq!(ScalePreference::default(), ScalePreference::System);
        for percent in SCALE_STEPS {
            assert!(ScalePreference::Percent(percent).is_valid());
            assert_eq!(ScalePreference::Percent(percent).resolve(133), percent);
        }
        for percent in [0, 74, 101, 133, 301, u16::MAX] {
            assert!(!ScalePreference::Percent(percent).is_valid());
            assert_eq!(
                ScalePreference::Percent(percent).validated(),
                ScalePreference::System
            );
        }
        assert_eq!(ScalePreference::System.resolve(0), 75);
        assert_eq!(ScalePreference::System.resolve(133), 133);
        assert_eq!(ScalePreference::System.resolve(u16::MAX), 300);
    }

    #[test]
    fn scale_steps_cover_explicit_sizes_and_nonstep_system_values() {
        for pair in SCALE_STEPS.windows(2) {
            assert_eq!(next_step(pair[0], true), pair[1]);
            assert_eq!(next_step(pair[1], false), pair[0]);
        }
        assert_eq!(next_step(75, false), 75);
        assert_eq!(next_step(300, true), 300);
        assert_eq!(next_step(133, false), 125);
        assert_eq!(next_step(133, true), 150);
    }
}
