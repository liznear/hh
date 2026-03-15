use std::path::PathBuf;

#[test]
fn app_modules_only_import_hh_widgets_via_widgets_adapter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let app_dir = root.join("src").join("app");
    let adapter_dir = root.join("src").join("app").join("widgets_adapter");

    let entries = walk_rs_files(&app_dir);
    for file in entries {
        let rel = file
            .strip_prefix(&root)
            .expect("path under workspace root")
            .to_string_lossy()
            .to_string();

        if rel.starts_with("src/app/widgets_adapter/") {
            continue;
        }

        if !adapter_dir.exists()
            && (rel == "src/app/render.rs"
                || rel == "src/app/components/messages.rs"
                || rel == "src/app/components/popups.rs")
        {
            continue;
        }

        let content = std::fs::read_to_string(&file).expect("read rust source");
        assert!(
            !content.contains("hh_widgets::"),
            "non-adapter module directly imports hh_widgets: {rel}"
        );
    }
}

fn walk_rs_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.clone()];

    while let Some(path) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&path) else {
            continue;
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    out
}
