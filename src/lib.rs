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
pub mod jellyfin_api;

pub use collection::Collection;
use collection::Error;
use collection::PathnameHash;
use templates::MISSING_POSTER;

/// Admin dashboard for managing your Jellyfin collection
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[clap(short, long, default_value = "127.0.0.1:3000")]
    pub bind: String,
    #[clap(short, long, env)]
    pub movie_dir: String,
    #[clap(short, long, env)]
    pub tag_dir: String,
    #[clap(short, long, default_value = "info")]
    pub log_level: tracing::Level,
    #[clap(short = 'j', long, env)]
    pub jellyfin_base_url: String,
    #[clap(short = 'a', long, env)]
    pub jellyfin_api_key: String,
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

#[derive(Debug, Clone)]
pub struct AppState {
    collection: Arc<RwLock<Collection>>,
    jellyfin_api: Arc<jellyfin_api::JellyfinClient>,
}

impl AppState {
    pub fn new(collection: Collection, jellyfin_api: jellyfin_api::JellyfinClient) -> Self {
        Self {
            collection: Arc::new(RwLock::new(collection)),
            jellyfin_api: Arc::new(jellyfin_api),
        }
    }
    
}

pub fn router(state: AppState) -> anyhow::Result<Router> {
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
    let router = Router::new()
        .route("/", get(routes::index))
        .route("/movies", get(routes::movie_list))
        .route("/movie/:id/poster.jpg", get(routes::movie_poster))
        .route("/movie/:id", get(routes::movie))
        .route("/movie/:id/tag/:tag", post(routes::toggle_tag))
        .route("/user-libraries", get(routes::user_libraries))
        .route("/user/:user_id/library/:folder_id", post(routes::toggle_user_library))
        .route("/reload", post(routes::reload))
        .nest_service("/static", ServeDir::new("src/static"))
        .layer(trace_layer)
        .with_state(state);
    Ok(router)
}

mod routes {
    use super::*;
    use axum::body::Body;
    use axum::extract::Path as PathExtractor;
    use axum::extract::Query;
    use axum::extract::State;
    use axum::response::Response;
    use maud::html;
    use maud::Markup;

    //#[tracing::instrument]
    pub async fn index(State(state): State<AppState>, Query(paging): Query<OptionalPaging>) -> impl IntoResponse {
        templates::index(&*state.collection.read().await, paging.into())
    }

    //#[tracing::instrument]
    pub async fn movie_poster(
        State(state): State<AppState>,
        PathExtractor(id): PathExtractor<String>,
    ) -> Result<Response, Error> {
        let hash = PathnameHash::from_str(&id)?;
        let collection = state.collection.read().await;
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
        State(state): State<AppState>,
        PathExtractor(id): PathExtractor<String>,
    ) -> Result<Markup, Error> {
        let hash = PathnameHash::from_str(&id)?;
        let collection = state.collection.read().await;
        let movie = collection.movies.get(&hash).ok_or(Error::NotFound)?;
        Ok(templates::movie(&collection, movie))
    }

    pub async fn toggle_tag(
        State(state): State<AppState>,
        PathExtractor((id, tag)): PathExtractor<(String, String)>,
    ) -> Result<Markup, Error> {
        let hash = PathnameHash::from_str(&id)?;
        let mut collection = state.collection.write().await;
        let movie = collection.movies.get(&hash).ok_or(Error::NotFound)?.clone();
        collection.toggle_tag(&tag, &movie).await?;
        Ok(templates::movie(&collection, &movie))
    }

    pub async fn reload(
        State(state): State<AppState>,
    ) -> Result<Response, Error> {
        let mut collection = state.collection.write().await;
        collection.reload().await?;
        let response = Response::builder()
            .status(303)
            .header("location", "/")
            .body(Body::empty())
            .unwrap();
        Ok(response)
    }

    pub async fn user_libraries(
        State(state): State<AppState>,
    ) -> Result<Markup, Error> {
        let users = state.jellyfin_api.get_users().await?;
        let folders = state.jellyfin_api.get_media_folders().await?;
        templates::user_libraries_page(&users, &folders)
    }

    pub async fn toggle_user_library(
        State(state): State<AppState>,
        PathExtractor((user_id, folder_id)): PathExtractor<(String, String)>,
    ) -> Result<Markup, Error> {
        let api1 = Arc::unwrap_or_clone(state.jellyfin_api.clone());
        let api2 = Arc::unwrap_or_clone(state.jellyfin_api.clone());
        let users_handle = tokio::spawn(async move {api1.get_users().await});
        let folders_handle = tokio::spawn(async move {api2.get_media_folders().await});
        let users: Vec<jellyfin_api::User> = users_handle.await.map_err(anyhow::Error::from)??;
        let folders: Vec<jellyfin_api::MediaFolders> = folders_handle.await.map_err(anyhow::Error::from)??;

        let mut user = users.iter().find(|u| u.id == user_id).ok_or(Error::NotFound)?.clone();
        let mut user_folders = user.enabled_folders()?;
        if user_folders.contains(&folder_id) {
            user_folders.retain(|f| f != &folder_id);
        } else {
            user_folders.push(folder_id);
        }
        tracing::debug!("Setting user folders: {:?}", &user_folders);
        state.jellyfin_api.set_user_media_folders(&user, &user_folders).await?;
        user.policy["EnabledFolders"] = serde_json::to_value(&user_folders)?;
        templates::user_libraries_entry(&user, &folders)
    }

    pub async fn movie_list(
        State(state): State<AppState>,
        Query(paging): Query<OptionalPaging>,
    ) -> Markup {
        let collection = state.collection.read().await;
        templates::movie_list(&collection, paging.into())
    }
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, Eq, PartialEq)]
pub struct OptionalPaging {
    page: Option<usize>,
    per_page: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub struct Paging {
    page: usize,
    per_page: usize,
}

impl From<OptionalPaging> for Paging {
    fn from(paging: OptionalPaging) -> Self {
        let default = Self::default();
        Self {
            page: paging.page.unwrap_or(default.page),
            per_page: paging.per_page.unwrap_or(default.per_page),
        }
    }
}

impl Default for Paging {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: 50,
        }
    }
}

impl Paging {
    pub fn offset(&self) -> usize {
        self.page.saturating_sub(1) * self.per_page
    }

    pub fn last_page(&self, total: usize) -> usize {
        (total + self.per_page - 1) / self.per_page
    }
}
