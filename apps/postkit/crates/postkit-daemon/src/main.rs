#[tokio::main]
async fn main() -> anyhow::Result<()> { postkit_daemon::run().await }
