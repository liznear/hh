use iocraft::prelude::*;

pub async fn run_iocraft_app(
    _settings: crate::config::Settings,
    _cwd: std::path::PathBuf,
) -> anyhow::Result<()> {
    element!(super::layout::AppRoot)
        .fullscreen()
        .await?;
    Ok(())
}
