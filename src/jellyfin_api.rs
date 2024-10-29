use reqwest::{IntoUrl, Method};
use serde::{Deserialize, Serialize};

use crate::collection::Error;


#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
struct APIList<T> {
    items: Vec<T>,
    total_record_count: usize,
    start_index: usize,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct User {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) policy: serde_json::Value,
}

impl User {
    pub(crate) fn is_admin(&self) -> Option<bool> {
        self.policy["IsAdministrator"].as_bool()
    }

    pub(crate) fn is_disabled(&self) -> Option<bool> {
        self.policy["IsDisabled"].as_bool()
    }

    pub(crate) fn enabled_folders(&self) -> Result<Vec<String>, serde_json::Error> {
        serde_json::from_value(self.policy["EnabledFolders"].clone())
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct MediaFolders {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) collection_type: String,
    pub(crate) etag: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JellyfinClient {
    base_url: String,
    api_key: String,
}

impl JellyfinClient {
    pub fn new(mut base_url: String, api_key: String) -> Self {
        if base_url.ends_with('/') {
            base_url.pop();
        }
        Self { base_url, api_key }
    }

    fn base_request(
        &self,
        client: &reqwest::Client,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, Error> {
        if !path.starts_with('/') {
            return Err(Error::InvalidPath(format!(
                "Path must start with \"/\": {path}"
            )));
        }
        let url = format!("{}{}", self.base_url, path);
        Ok(client
            .request(method, url)
            .header(
                "Authorization",
                format!(r#"MediaBrowser Token="{}""#, self.api_key),
            )
            .header("Accept", "application/json"))
    }

    #[tracing::instrument]
    pub(crate) async fn get_users(&self) -> Result<Vec<User>, Error> {
        tracing::debug!("Getting users");
        let resp = self.base_request(&reqwest::Client::new(), Method::GET, "/Users")?.send().await?;
        let text = resp.text().await?;
        tracing::debug!("Users response: {}", text);
        let users: Vec<User> = serde_json::from_str(&text)?;
        Ok(users)
    }

    #[tracing::instrument]
    pub(crate) async fn get_media_folders(&self) -> Result<Vec<MediaFolders>, Error> {
        tracing::debug!("Getting media folders");
        let resp = self
            .base_request(&reqwest::Client::new(), Method::GET, "/Library/MediaFolders")?
            .send()
            .await?;
        let text = resp.text().await?;
        let folders: APIList<MediaFolders> = serde_json::from_str(&text)?;
        Ok(folders.items)
    }

    #[tracing::instrument]
    pub(crate) async fn set_user_media_folders(&self, user: &User, folders: &[String]) -> Result<(), Error> {
        let mut policy = user.policy.clone();
        policy["EnabledFolders"] = serde_json::to_value(folders)?;
        let path = format!("/Users/{}/Policy", user.id);
        let resp = self
            .base_request(&reqwest::Client::new(), Method::POST, &path)?
            .json(&policy)
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(Error::JellyfinError(format!(
                "Failed to set media folders: User id = {}, {}",
                user.id,
                resp.text().await?,
            )))
        }
    }
}

fn to_json_array<T: Serialize>(items: &[T]) -> Result<serde_json::Value, serde_json::Error> {
    let mut values = Vec::new();
    for item in items {
        values.push(serde_json::to_value(item)?);
    }
    Ok(serde_json::Value::Array(values))
}
