use std::str::FromStr;
use std::sync::Arc;

use axum::http::Request;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use axum_insights::AppInsightsError;
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

mod collection;
mod templates;

pub use collection::Collection;
use collection::Error;
use collection::PathnameHash;
use templates::MISSING_POSTER;

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(short, long, default_value = "127.0.0.1:3000")]
    pub bind: String,
    #[clap(short, long)]
    pub movie_dir: String,
    #[clap(short, long)]
    pub tag_dir: String,
    #[clap(short, long, default_value = "info")]
    pub log_level: tracing::Level,
}

#[derive(Default, Serialize, Deserialize, Clone)]
struct WebError {
    message: String,
}

impl IntoResponse for WebError {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        axum::http::Response::builder()
            .status(400)
            .header("content-type", "text/plain")
            .body(self.message.into())
            .unwrap()
    }
}

impl AppInsightsError for WebError {
    fn message(&self) -> Option<String> {
        Some(self.message.clone())
    }

    fn backtrace(&self) -> Option<String> {
        None
    }
}

pub fn router(collection: Collection) -> anyhow::Result<Router> {
    let trace_layer = TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
        let request_id = uuid::Uuid::new_v4();
        tracing::info_span!(
            "request",
            %request_id,
            method = ?req.method(),
            uri = %req.uri(),
            version = ?req.version(),
        )
    });
    let collection_state = Arc::new(RwLock::new(collection));
    let router = Router::new()
        .route("/", get(routes::index))
        .route("/movie/:id/poster.jpg", get(routes::movie_poster))
        .route("/movie/:id", get(routes::movie))
        .route("/movie/:id/tag/:tag", post(routes::toggle_tag))
        .route("/reload", post(routes::reload))
        .nest_service("/static", ServeDir::new("src/static"))
        .layer(trace_layer)
        .with_state(collection_state);
    Ok(router)
}

mod routes {
    use super::*;
    use axum::body::Body;
    use axum::extract::Path as PathExtractor;
    use axum::extract::State;
    use axum::response::Response;
    use maud::Markup;

    //#[tracing::instrument]
    pub async fn index(State(collection): State<Arc<RwLock<Collection>>>) -> impl IntoResponse {
        templates::index(&*collection.read().await)
    }

    //#[tracing::instrument]
    pub async fn movie_poster(
        State(collection): State<Arc<RwLock<Collection>>>,
        PathExtractor(id): PathExtractor<String>,
    ) -> Result<Response, Error> {
        let hash = PathnameHash::from_str(&id)?;
        let collection = collection.read().await;
        let movie = collection.movies.get(&hash).unwrap();
        let body = match &movie.poster_path {
            Some(poster_path) => {
                let metadata = tokio::fs::metadata(poster_path).await?;
                let mut file = tokio::fs::File::open(poster_path).await?;
                let mut image_data = Vec::with_capacity(metadata.len() as usize);
                file.read_to_end(&mut image_data).await?;
                Body::from(image_data)
            }
            None => Body::from(MISSING_POSTER),
        };
        let response = Response::builder()
            .header("content-type", "image/jpeg")
            .body(body)
            .unwrap();
        Ok(response)
    }

    //#[tracing::instrument]
    pub async fn movie(
        State(collection): State<Arc<RwLock<Collection>>>,
        PathExtractor(id): PathExtractor<String>,
    ) -> Result<Markup, Error> {
        let hash = PathnameHash::from_str(&id)?;
        let collection = collection.read().await;
        let movie = collection.movies.get(&hash).ok_or(Error::NotFound)?;
        Ok(templates::movie(&collection, movie))
    }

    pub async fn toggle_tag(
        State(collection): State<Arc<RwLock<Collection>>>,
        PathExtractor((id, tag)): PathExtractor<(String, String)>,
    ) -> Result<Markup, Error> {
        let hash = PathnameHash::from_str(&id)?;
        let mut collection = collection.write().await;
        let movie = collection.movies.get(&hash).ok_or(Error::NotFound)?.clone();
        collection.toggle_tag(&tag, &movie).await?;
        Ok(templates::movie(&collection, &movie))
    }

    pub async fn reload(
        State(collection): State<Arc<RwLock<Collection>>>,
    ) -> Result<Response, Error> {
        let mut collection = collection.write().await;
        collection.reload().await?;
        let response = Response::builder()
            .status(303)
            .header("location", "/")
            .body(Body::empty())
            .unwrap();
        Ok(response)
    }
}
