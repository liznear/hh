use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct MyProps {
    pub name: String,
}

#[component]
fn MyComponent(props: &MyProps) -> impl Into<AnyElement<'static>> {
    element!(Text(content: format!("Hello, {}!", props.name)))
}
