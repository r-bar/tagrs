use axum::http::StatusCode;
use axum::response::IntoResponse;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use tokio::fs::read_dir;

type Tags = HashMap<String, HashSet<PathnameHash>>;
type Movies = HashMap<PathnameHash, Movie>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub(crate) struct PathnameHash([u8; 20]);

impl FromStr for PathnameHash {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(hex::decode(s)?.as_slice())
    }
}

impl PathnameHash {
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<&[u8]> for PathnameHash {
    type Error = Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != 20 {
            return Err(anyhow::anyhow!("invalid hash length").into());
        }
        let mut hash = [0; 20];
        hash.copy_from_slice(value);
        Ok(PathnameHash(hash))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Movie {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) hash: PathnameHash,
    pub(crate) poster_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub(crate) tags: Tags,
    pub(crate) movies: Movies,
    pub(crate) movie_dir: PathBuf,
    pub(crate) tag_dir: PathBuf,
}

impl Display for Collection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Collection {{ movie_dir: {}, tag_dir: {}, tag_count: {}, movie_count: {} }}", self.movie_dir.display(), self.tag_dir.display(), self.tags.len(), self.movies.len())
    }
}

fn path_hash<T>(path: T) -> anyhow::Result<PathnameHash>
where
    T: AsRef<Path>,
{
    let mut hasher = Sha1::new();
    let pathstr = path
        .as_ref()
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid file name: {}", path.as_ref().to_string_lossy()))?
        .as_encoded_bytes();
    //tracing::debug!("hashing path: {:?}", pathstr);
    hasher.update(pathstr);
    Ok(PathnameHash(hasher.finalize().into()))
}

impl Collection {
    pub async fn new<T>(movie_dir: T, tag_dir: T) -> anyhow::Result<Self>
    where
        T: AsRef<Path> + Eq + std::hash::Hash,
    {
        let mut ignore_paths = HashSet::new();
        let abs_movie_dir = tokio::fs::canonicalize(movie_dir.as_ref()).await?;
        let abs_tag_dir = tokio::fs::canonicalize(tag_dir.as_ref()).await?;
        ignore_paths.insert(abs_movie_dir.clone());
        Ok(Collection {
            movies: Self::load_movies(&movie_dir).await?,
            tags: Self::load_tags(&abs_tag_dir, &ignore_paths).await?,
            movie_dir: abs_movie_dir,
            tag_dir: abs_tag_dir,
        })
    }

    async fn load_movies<T>(movie_dir: T) -> anyhow::Result<Movies>
    where
        T: AsRef<Path>,
    {
        //tracing::debug!("loading movies from {:?}", movie_dir.as_ref());
        let mut movies = HashMap::new();
        let mut entries = read_dir(&movie_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            //tracing::debug!("entry: {:?}", entry);
            if entry.file_type().await?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();
                let hash = path_hash(&path)?;
                let possible_poster_path = path.join("poster.jpg");
                let poster_path = if possible_poster_path.exists() {
                    Some(possible_poster_path)
                } else {
                    None
                };
                let movie = Movie {
                    name,
                    hash,
                    path,
                    poster_path,
                };
                movies.insert(hash, movie);
            }
        }
        Ok(movies)
    }

    async fn load_tags<D>(tag_index_dir: D, ignore: &HashSet<PathBuf>) -> anyhow::Result<Tags>
    where
        D: AsRef<Path>,
    {
        let mut tags = HashMap::new();
        let mut entries = read_dir(&tag_index_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                if ignore.contains(&entry.path()) {
                    continue;
                }
                let tag = entry
                    .file_name()
                    .to_str()
                    .ok_or(anyhow::anyhow!("Invalid tag directory name"))?
                    .to_string();
                tags.insert(tag, HashSet::new());
            }
        }

        for (tag, movie_tags) in tags.iter_mut() {
            let tag_dir = tag_index_dir.as_ref().join(tag);
            let mut dir_entries = read_dir(&tag_dir).await?;
            while let Some(entry) = dir_entries.next_entry().await? {
                if entry.file_type().await?.is_symlink() {
                    let hash = path_hash(entry.path())?;
                    movie_tags.insert(hash);
                }
            }
        }

        Ok(tags)
    }

    pub(crate) async fn toggle_tag(&mut self, tag: &str, movie: &Movie) -> Result<(), Error> {
        let tag_movies = self.tags.get_mut(tag).ok_or(Error::NotFound)?;
        let tag_path = self.tag_dir.join(tag).join(movie.path.file_name().unwrap());
        let movie_path = self.movie_dir.join(movie.path.file_name().unwrap());
        if tag_movies.contains(&movie.hash) {
            tracing::debug!("unlinking {} from {}", tag_path.display(), movie.path.display());
            tokio::fs::remove_file(&tag_path).await?;
            tag_movies.remove(&movie.hash);
        } else {
            tracing::debug!("linking {} to {}", movie.path.display(), tag_path.display());
            tokio::fs::symlink(movie_path, &tag_path).await?;
            tag_movies.insert(movie.hash);
        }
        Ok(())
    }

    pub(crate) async fn reload(&mut self) -> Result<(), Error> {
        self.movies = Self::load_movies(&self.movie_dir).await?;
        let mut ignore_paths = HashSet::new();
        ignore_paths.insert(self.movie_dir.clone());
        self.tags = Self::load_tags(&self.tag_dir, &ignore_paths).await?;
        tracing::debug!("Reloaded collections: {}", self);
        Ok(())
    }
}

impl Movie {
    pub(crate) fn id(&self) -> String {
        hex::encode(self.hash.as_slice())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    NotFound,
    IO(std::io::Error),
    Other(anyhow::Error),
    JellyfinError(String),
    InvalidPath(String),
    JsonEncodingError(serde_json::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::JellyfinError(msg) => write!(f, "Jellyfin error: {}", msg),
            Error::InvalidPath(msg) => write!(f, "Invalid path: {}", msg),
            Error::IO(e) => write!(f, "IO error: {}", e),
            Error::Other(e) => write!(f, "{}", e),
            Error::NotFound => write!(f, "Not found"),
            Error::JsonEncodingError(e) => write!(f, "Json encoding error: {}", e),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        match self {
            Error::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
            Error::IO(e) => {
                tracing::error!("io error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("IO error: {}", e)
                )
                    .into_response()
            }
            Error::Other(e) => {
                tracing::error!("error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Internal server error: {}", e)
                )
                    .into_response()
            }
            Error::JellyfinError(e) => {
                tracing::error!("jellyfin error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Jellyfin error: {}", e)
                )
                    .into_response()
            }
            Error::InvalidPath(e) => {
                tracing::error!("invalid path: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Invalid path: {}", e)
                )
                    .into_response()
            }
            Error::JsonEncodingError(e) => {
                tracing::error!("json encoding error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Json encoding error: {}", e)
                )
                    .into_response()
            }
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Other(e)
    }
}

impl From<hex::FromHexError> for Error {
    fn from(e: hex::FromHexError) -> Self {
        Error::Other(e.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::JellyfinError(format!("{}", e))
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::JsonEncodingError(e)
    }
}
