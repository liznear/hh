#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hh_cli::cli::run().await
}
