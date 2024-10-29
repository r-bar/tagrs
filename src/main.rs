use tagrs::{Collection, Cli, router, jellyfin_api, AppState};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    tracing_subscriber::fmt().with_max_level(args.log_level).with_target(false).init();
    let collection = Collection::new(&args.movie_dir, &args.tag_dir).await?;
    tracing::debug!("{}", &collection);
    let jellyfin_api = jellyfin_api::JellyfinClient::new(args.jellyfin_base_url, args.jellyfin_api_key);
    tracing::debug!("{:?}", &jellyfin_api);
    let state = AppState::new(collection, jellyfin_api);
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    tracing::info!("Starting server on {}", args.bind);
    axum::serve(listener, router(state)?).await?;
    Ok(())
}
