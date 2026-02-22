#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hh::cli::run().await
}
