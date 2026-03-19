use iocraft::prelude::Color;

/// Base16 theme colors
/// See: https://github.com/chriskempson/base16
///
/// base00-base07: Gradients (typically dark to light for dark themes, light to dark for light themes)
/// base08-base0F: Semantic colors (red, green, yellow, blue, magenta, cyan, orange, violet/brown)
#[derive(Clone, Debug)]
#[allow(non_snake_case)]
#[allow(dead_code)]
pub struct Theme {
    // Base gradients
    pub base00: Color,
    pub base01: Color,
    pub base02: Color,
    pub base03: Color,
    pub base04: Color,
    pub base05: Color,
    pub base06: Color,
    pub base07: Color,

    // Semantic colors
    pub base08: Color, // Red
    pub base09: Color, // Orange
    pub base0A: Color, // Yellow
    pub base0B: Color, // Green
    pub base0C: Color, // Cyan
    pub base0D: Color, // Blue
    pub base0E: Color, // Magenta/Violet
    pub base0F: Color, // Brown
}

impl Theme {
    #[allow(dead_code)]
    /// Default light theme matching current TUI colors
    pub fn light() -> Self {
        Self {
            // Light theme: base00 is lightest, base07 is darkest
            base00: Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            }, // White (main background)
            base01: Color::Rgb {
                r: 250,
                g: 250,
                b: 250,
            }, // Off-white
            base02: Color::Rgb {
                r: 220,
                g: 220,
                b: 220,
            }, // Light grey (sidebar)
            base03: Color::Rgb {
                r: 180,
                g: 180,
                b: 180,
            }, // Mid grey
            base04: Color::Rgb {
                r: 120,
                g: 120,
                b: 120,
            }, // Dark grey
            base05: Color::Rgb {
                r: 60,
                g: 60,
                b: 60,
            }, // Darker grey
            base06: Color::Rgb {
                r: 30,
                g: 30,
                b: 30,
            }, // Very dark grey
            base07: Color::Rgb { r: 0, g: 0, b: 0 }, // Black

            // Semantic colors (standard base16 colors)
            base08: Color::Rgb {
                r: 224,
                g: 49,
                b: 49,
            }, // Red
            base09: Color::Rgb {
                r: 230,
                g: 126,
                b: 34,
            }, // Orange
            base0A: Color::Rgb {
                r: 241,
                g: 196,
                b: 15,
            }, // Yellow
            base0B: Color::Rgb {
                r: 39,
                g: 174,
                b: 96,
            }, // Green
            base0C: Color::Rgb {
                r: 26,
                g: 188,
                b: 156,
            }, // Cyan
            base0D: Color::Rgb {
                r: 52,
                g: 152,
                b: 219,
            }, // Blue
            base0E: Color::Rgb {
                r: 155,
                g: 89,
                b: 182,
            }, // Violet
            base0F: Color::Rgb {
                r: 142,
                g: 68,
                b: 173,
            }, // Brown
        }
    }

    /// Default dark theme
    #[allow(dead_code)]
    pub fn dark() -> Self {
        Self {
            // Dark theme: base00 is darkest, base07 is lightest
            base00: Color::Rgb {
                r: 40,
                g: 42,
                b: 54,
            }, // Background
            base01: Color::Rgb {
                r: 44,
                g: 46,
                b: 58,
            }, // Lighter bg
            base02: Color::Rgb {
                r: 60,
                g: 62,
                b: 74,
            }, // Selection bg
            base03: Color::Rgb {
                r: 98,
                g: 100,
                b: 112,
            }, // Comments
            base04: Color::Rgb {
                r: 130,
                g: 132,
                b: 144,
            }, // Dark fg
            base05: Color::Rgb {
                r: 200,
                g: 202,
                b: 210,
            }, // Default fg
            base06: Color::Rgb {
                r: 230,
                g: 232,
                b: 240,
            }, // Light fg
            base07: Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            }, // Light bg

            // Semantic colors
            base08: Color::Rgb {
                r: 255,
                g: 85,
                b: 85,
            }, // Red
            base09: Color::Rgb {
                r: 255,
                g: 121,
                b: 68,
            }, // Orange
            base0A: Color::Rgb {
                r: 241,
                g: 250,
                b: 140,
            }, // Yellow
            base0B: Color::Rgb {
                r: 80,
                g: 250,
                b: 123,
            }, // Green
            base0C: Color::Rgb {
                r: 139,
                g: 233,
                b: 253,
            }, // Cyan
            base0D: Color::Rgb {
                r: 98,
                g: 175,
                b: 239,
            }, // Blue
            base0E: Color::Rgb {
                r: 189,
                g: 147,
                b: 249,
            }, // Violet
            base0F: Color::Rgb {
                r: 255,
                g: 121,
                b: 198,
            }, // Pink/Brown
        }
    }
}

/// Semantic color accessors for common UI elements
#[allow(dead_code)]
impl Theme {
    /// Main background color
    pub fn background(&self) -> Color {
        self.base00
    }

    /// Secondary/suted background (sidebars, panels)
    pub fn background_secondary(&self) -> Color {
        self.base01
    }

    /// Tertiary background (selections, highlights)
    pub fn background_tertiary(&self) -> Color {
        self.base02
    }

    /// Default text/foreground color
    pub fn foreground(&self) -> Color {
        self.base07
    }

    /// Secondary/muted text
    pub fn foreground_secondary(&self) -> Color {
        self.base06
    }

    /// Tertiary/subtle text
    pub fn foreground_tertiary(&self) -> Color {
        self.base05
    }

    /// Error/accent red
    pub fn error(&self) -> Color {
        self.base08
    }

    /// Warning/accent yellow
    pub fn warning(&self) -> Color {
        self.base0A
    }

    /// Success/accent green
    pub fn success(&self) -> Color {
        self.base0B
    }

    /// Info/accent blue
    pub fn info(&self) -> Color {
        self.base0D
    }
}

/// Global theme instance
/// TODO: Support dynamic theme switching via config
pub fn current_theme() -> Theme {
    Theme::light()
}
