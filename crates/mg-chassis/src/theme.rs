//! Browser-control colors only. The host supplies the desktop's preferred scheme;
//! document styling and page pixels never depend on this preference.

/// The user's browser-control appearance preference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// A resolved color scheme, supplied by the host or explicitly selected.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

impl ThemePreference {
    pub(super) fn resolve(self, system: ColorScheme) -> ColorScheme {
        match self {
            Self::System => system,
            Self::Light => ColorScheme::Light,
            Self::Dark => ColorScheme::Dark,
        }
    }
}

#[cfg(feature = "chrome")]
#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub background: u32,
    pub button: u32,
    pub field: u32,
    pub selection: u32,
    pub ink: u32,
    pub muted: u32,
    pub accent: u32,
    pub border: u32,
    pub panel: u32,
    pub scrollbar: u32,
}

#[cfg(feature = "chrome")]
impl ColorScheme {
    pub(super) fn palette(self) -> Palette {
        match self {
            Self::Light => Palette {
                background: 0xeef1e9,
                button: 0xe3e9df,
                field: 0xffffff,
                selection: 0xc6dfed,
                ink: 0x26342b,
                muted: 0x53614f,
                accent: 0x345c36,
                border: 0xc4cebd,
                panel: 0xfafbf8,
                scrollbar: 0x8b9c83,
            },
            Self::Dark => Palette {
                background: 0x202622,
                button: 0x323c35,
                field: 0x151b17,
                selection: 0x365365,
                ink: 0xe7eee6,
                muted: 0xb6c5b5,
                accent: 0xa7dba6,
                border: 0x59675b,
                panel: 0x252e28,
                scrollbar: 0x80967f,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_preferences_override_both_system_schemes() {
        for system in [ColorScheme::Light, ColorScheme::Dark] {
            assert_eq!(ThemePreference::System.resolve(system), system);
            assert_eq!(ThemePreference::Light.resolve(system), ColorScheme::Light);
            assert_eq!(ThemePreference::Dark.resolve(system), ColorScheme::Dark);
        }
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn palette_text_has_readable_contrast() {
        fn luminance(rgb: u32) -> f64 {
            let channel = |shift: u32| {
                let value = f64::from((rgb >> shift) & 255u32) / 255.;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
        }
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            let p = scheme.palette();
            for (foreground, background) in [
                (p.ink, p.button),
                (p.ink, p.field),
                (p.ink, p.selection),
                (p.ink, p.panel),
                (p.muted, p.background),
                (p.muted, p.panel),
                (p.accent, p.background),
            ] {
                let a = luminance(foreground);
                let b = luminance(background);
                assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
            }
        }
    }
}
