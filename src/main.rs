use anyhow::Result;
use bore_cli::cli::{run, Args};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    run(args).await
}
