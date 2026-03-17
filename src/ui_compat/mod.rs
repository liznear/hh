
pub mod backend {
    pub struct CrosstermBackend<W>(std::marker::PhantomData<W>);
    impl<W> CrosstermBackend<W> {
        pub fn new(_w: W) -> Self { Self(std::marker::PhantomData) }
    }
}
pub mod layout {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Rect {
        pub x: u16,
        pub y: u16,
        pub width: u16,
        pub height: u16,
    }
    impl Rect {
        pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self { Self { x, y, width, height } }
        pub fn right(&self) -> u16 { self.x.saturating_add(self.width) }
        pub fn bottom(&self) -> u16 { self.y.saturating_add(self.height) }
        pub fn inset(&self, px: u16, py: u16) -> Self {
            Self {
                x: self.x.saturating_add(px),
                y: self.y.saturating_add(py),
                width: self.width.saturating_sub(px.saturating_mul(2)),
                height: self.height.saturating_sub(py.saturating_mul(2)),
            }
        }
    }
    
    #[derive(Clone, Copy, Debug)]
    pub enum Constraint {
        Length(u16),
        Min(u16),
        Max(u16),
        Percentage(u16),
    }
    
    #[derive(Clone, Copy, Debug)]
    pub enum Direction {
        Horizontal,
        Vertical,
    }
    
    pub struct Layout;
    impl Layout {
        #[allow(clippy::should_implement_trait)]
        pub fn default() -> Self { Self }
        pub fn direction(self, _d: Direction) -> Self { self }
        pub fn constraints(self, _c: &[Constraint]) -> Self { self }
        pub fn split(self, r: Rect) -> std::rc::Rc<[Rect]> {
            std::rc::Rc::new([r, r, r, r, r, r, r, r])
        }
    }
}
pub mod style {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Color {
        Reset, Black, Red, Green, Yellow, Blue, Magenta, Cyan, Gray, DarkGray,
        LightRed, LightGreen, LightYellow, LightBlue, LightMagenta, LightCyan, White,
        Rgb(u8, u8, u8), Indexed(u8),
    }

    bitflags::bitflags! {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct Modifier: u16 {
            const BOLD              = 0b0001;
            const DIM               = 0b0010;
            const ITALIC            = 0b0100;
            const UNDERLINED        = 0b1000;
            const SLOW_BLINK        = 0b0001_0000;
            const RAPID_BLINK       = 0b0010_0000;
            const REVERSED          = 0b0100_0000;
            const HIDDEN            = 0b1000_0000;
            const CROSSED_OUT       = 0b0001_0000_0000;
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Style {
        pub fg: Option<Color>,
        pub bg: Option<Color>,
        pub add_modifier: Modifier,
        pub sub_modifier: Modifier,
    }
    
    impl Style {
        pub fn new() -> Self { Self::default() }
        pub fn fg(mut self, color: Color) -> Self { self.fg = Some(color); self }
        pub fn bg(mut self, color: Color) -> Self { self.bg = Some(color); self }
        pub fn add_modifier(mut self, modifier: Modifier) -> Self { self.add_modifier |= modifier; self }
        pub fn remove_modifier(mut self, modifier: Modifier) -> Self { self.sub_modifier |= modifier; self }
        
        pub fn bold(self) -> Self { self.add_modifier(Modifier::BOLD) }
        pub fn italic(self) -> Self { self.add_modifier(Modifier::ITALIC) }
        pub fn dim(self) -> Self { self.add_modifier(Modifier::DIM) }
        pub fn underlined(self) -> Self { self.add_modifier(Modifier::UNDERLINED) }
        
        pub fn patch(mut self, other: Style) -> Self {
            if let Some(fg) = other.fg { self.fg = Some(fg); }
            if let Some(bg) = other.bg { self.bg = Some(bg); }
            self.add_modifier |= other.add_modifier;
            self.sub_modifier |= other.sub_modifier;
            self
        }
    }
}
pub mod text {
    use std::borrow::Cow;
        use super::style::Style;
    
    #[derive(Clone, Debug, PartialEq)]
    pub struct Span<'a> {
        pub content: Cow<'a, str>,
        pub style: Style,
    }
    
    impl<'a> Span<'a> {
        pub fn new(content: impl Into<Cow<'a, str>>, style: Style) -> Self {
            Self { content: content.into(), style }
        }
        pub fn raw(content: impl Into<Cow<'a, str>>) -> Self {
            Self { content: content.into(), style: Style::default() }
        }
        pub fn styled(content: impl Into<Cow<'a, str>>, style: Style) -> Self {
            Self::new(content, style)
        }
        pub fn width(&self) -> usize {
            unicode_width::UnicodeWidthStr::width(self.content.as_ref())
        }
    }
    
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Line<'a> {
        pub spans: Vec<Span<'a>>,
        pub style: Style,
    }
    
    impl<'a> Line<'a> {
        pub fn new() -> Self { Self::default() }
        pub fn raw(content: impl Into<Cow<'a, str>>) -> Self {
            Self { spans: vec![Span::raw(content)], style: Style::default() }
        }
        pub fn styled(content: impl Into<Cow<'a, str>>, style: Style) -> Self {
            Self { spans: vec![Span::styled(content, style)], style: Style::default() }
        }
        pub fn is_empty(&self) -> bool { self.spans.iter().all(|s| s.content.is_empty()) }
        pub fn width(&self) -> usize { self.spans.iter().map(|s| s.width()).sum() }
    }
    
    impl<'a> From<String> for Line<'a> { fn from(s: String) -> Self { Line::raw(s) } }
    impl<'a> From<&'a str> for Line<'a> { fn from(s: &'a str) -> Self { Line::raw(s) } }
    impl<'a> From<Vec<Span<'a>>> for Line<'a> { fn from(spans: Vec<Span<'a>>) -> Self { Self { spans, style: Style::default() } } }
    impl<'a> From<Span<'a>> for Line<'a> { fn from(span: Span<'a>) -> Self { Self { spans: vec![span], style: Style::default() } } }
    
    pub type Text<'a> = Vec<Line<'a>>;
}
pub mod widgets {
    use super::style::Style;
        
    #[derive(Clone, Copy)]
    pub struct Block;
    impl Block {
        #[allow(clippy::should_implement_trait)]
        pub fn default() -> Self { Self }
        pub fn style(self, _s: Style) -> Self { self }
        pub fn padding(self, _p: Padding) -> Self { self }
        pub fn inner(self, r: super::layout::Rect) -> super::layout::Rect { r }
    }
    #[derive(Clone, Copy)]
    pub struct Paragraph;
    impl Paragraph {
        pub fn new<T>(_t: T) -> Self { Self }
        pub fn style(self, _s: Style) -> Self { self }
        pub fn wrap(self, _w: Wrap) -> Self { self }
        pub fn scroll(self, _o: (u16, u16)) -> Self { self }
    }
    pub struct Wrap { pub trim: bool }
    
    pub struct Clear;
    pub struct List;
    impl List {
        pub fn new<T>(_t: T) -> Self { Self }
        pub fn style(self, _s: Style) -> Self { self }
    }
    pub struct ListItem;
    impl ListItem {
        pub fn new<T>(_t: T) -> Self { Self }
        pub fn style(self, _s: Style) -> Self { self }
    }
    pub struct Padding;
    impl Padding {
        pub fn new(_l: u16, _r: u16, _t: u16, _b: u16) -> Self { Self }
    }
}

pub struct Terminal<B>(std::marker::PhantomData<B>);
impl<B> Terminal<B> {
    pub fn new(_b: B) -> Result<Self, std::io::Error> {
        Ok(Self(std::marker::PhantomData))
    }
    pub fn size(&self) -> Result<layout::Rect, std::io::Error> {
        Ok(layout::Rect::default())
    }
    pub fn clear(&mut self) -> Result<(), std::io::Error> { Ok(()) }
    pub fn autoresize(&mut self) -> Result<(), std::io::Error> { Ok(()) }
}

pub struct Frame<'a>(std::marker::PhantomData<&'a ()>);
impl<'a> Frame<'a> {
    pub fn area(&self) -> layout::Rect {
        layout::Rect::default()
    }
    pub fn render_widget<W>(&mut self, _w: W, _r: layout::Rect) {}
    pub fn set_cursor_position(&mut self, _p: (u16, u16)) {}
}

pub mod prelude {
    pub use super::style::{Color, Style, Modifier};
    pub use super::text::{Line, Span, Text};
    pub use super::layout::Rect;
    pub use super::backend::CrosstermBackend;
    
    pub trait Stylize<'a, T>: Sized {
        fn fg(self, color: Color) -> T;
        fn bg(self, color: Color) -> T;
        fn bold(self) -> T;
        fn italic(self) -> T;
        fn dim(self) -> T;
        fn underlined(self) -> T;
    }
    
    impl<'a> Stylize<'a, Span<'a>> for Span<'a> {
        fn fg(mut self, color: Color) -> Self { self.style = self.style.fg(color); self }
        fn bg(mut self, color: Color) -> Self { self.style = self.style.bg(color); self }
        fn bold(mut self) -> Self { self.style = self.style.bold(); self }
        fn italic(mut self) -> Self { self.style = self.style.italic(); self }
        fn dim(mut self) -> Self { self.style = self.style.dim(); self }
        fn underlined(mut self) -> Self { self.style = self.style.underlined(); self }
    }
}
