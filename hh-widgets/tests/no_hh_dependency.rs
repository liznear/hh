use std::path::PathBuf;

#[test]
fn source_does_not_reference_hh_internal_modules() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = crate_root.join("src");
    let entries = std::fs::read_dir(&src_dir).expect("read src dir");

    for entry in entries {
        let entry = entry.expect("read src entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }

        let content = std::fs::read_to_string(&path).expect("read source file");
        assert!(
            !content.contains("crate::app")
                && !content.contains("crate::core")
                && !content.contains("crate::tool")
                && !content.contains("hh::")
                && !content.contains("hh_cli"),
            "forbidden internal dependency reference in {}",
            path.display()
        );
    }
}
