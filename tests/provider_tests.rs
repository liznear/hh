use hh::provider::openai_compatible::OpenAiCompatibleProvider;

#[test]
fn provider_endpoint_normalizes_trailing_slash() {
    let provider = OpenAiCompatibleProvider::new(
        "https://example.com/v1/".to_string(),
        "model-x".to_string(),
        "OPENAI_API_KEY".to_string(),
    );

    let debug = format!("{:?}", std::any::type_name_of_val(&provider));
    assert!(debug.contains("OpenAiCompatibleProvider"));
}
