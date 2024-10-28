use tagrs::{Collection, Cli, router};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    tracing_subscriber::fmt().with_max_level(args.log_level).with_target(false).init();
    let collection = Collection::new(&args.movie_dir, &args.tag_dir).await?;
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    tracing::debug!("{}", collection);
    tracing::info!("Starting server on {}", args.bind);
    axum::serve(listener, router(collection)?).await?;
    Ok(())
}
