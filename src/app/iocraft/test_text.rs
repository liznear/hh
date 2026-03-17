use iocraft::prelude::*;
fn test() -> impl Into<AnyElement<'static>> {
    element! {
        MixedText(contents: vec![
            MixedTextContent::new("Hello").color(Color::Red),
            MixedTextContent::new(" World").color(Color::Blue),
        ])
    }
}
