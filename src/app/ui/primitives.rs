#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl UiColor {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiStyle {
    pub fg: Option<UiColor>,
    pub bg: Option<UiColor>,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub underline: bool,
}

impl UiStyle {
    pub fn fg(mut self, color: UiColor) -> Self {
        self.fg = Some(color);
        self
    }

    pub fn bg(mut self, color: UiColor) -> Self {
        self.bg = Some(color);
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiSpan {
    pub content: String,
    pub style: UiStyle,
}

impl UiSpan {
    pub fn new(content: impl Into<String>, style: UiStyle) -> Self {
        Self {
            content: content.into(),
            style,
        }
    }

    pub fn raw(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: UiStyle::default(),
        }
    }

    pub fn styled(content: impl Into<String>, style: UiStyle) -> Self {
        Self::new(content, style)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiLine {
    pub spans: Vec<UiSpan>,
}

impl UiLine {
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    pub fn from(spans: Vec<UiSpan>) -> Self {
        Self { spans }
    }

    pub fn raw(content: impl Into<String>) -> Self {
        Self {
            spans: vec![UiSpan::raw(content)],
        }
    }

    pub fn styled(content: impl Into<String>, style: UiStyle) -> Self {
        Self {
            spans: vec![UiSpan::styled(content, style)],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|s| s.content.is_empty())
    }

    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.content.chars().count()).sum()
    }
}

impl From<String> for UiLine {
    fn from(s: String) -> Self {
        UiLine::raw(s)
    }
}

impl From<&str> for UiLine {
    fn from(s: &str) -> Self {
        UiLine::raw(s)
    }
}

impl From<Vec<UiSpan>> for UiLine {
    fn from(spans: Vec<UiSpan>) -> Self {
        UiLine::from(spans)
    }
}
