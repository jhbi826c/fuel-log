use gloo_net::http::{Request, RequestBuilder};
use leptos::prelude::*;
use serde_json::{Value, json};
use web_sys::RequestCache;

use crate::model::AppData;
use crate::store::Store;
use crate::web;

const API: &str = "https://api.github.com";
const FILE: &str = "fuel-log.json";

#[derive(Debug, Clone)]
pub enum SyncError {
    NoToken,
    Unauthorized,
    Http(u16, String),
    Network(String),
    BadRemote(String),
}

#[derive(Debug, Clone, Copy)]
pub struct SyncResult {
    pub pulled: usize,
}

fn authed(req: RequestBuilder, token: &str) -> RequestBuilder {
    req.header("Accept", "application/vnd.github+json")
        .header("Authorization", &format!("Bearer {token}"))
        .cache(RequestCache::NoStore)
}

async fn send(req: Request) -> Result<Value, SyncError> {
    let res = req.send().await.map_err(|e| SyncError::Network(e.to_string()))?;
    let status = res.status();
    if status == 401 || status == 403 {
        return Err(SyncError::Unauthorized);
    }
    if !res.ok() {
        let msg = res
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or_default();
        return Err(SyncError::Http(status, msg));
    }
    res.json::<Value>().await.map_err(|e| SyncError::Network(e.to_string()))
}

async fn get(path: &str, token: &str) -> Result<Value, SyncError> {
    let req = authed(Request::get(&format!("{API}{path}")), token)
        .build()
        .map_err(|e| SyncError::Network(e.to_string()))?;
    send(req).await
}

async fn write(method_patch: bool, path: &str, token: &str, body: &Value) -> Result<Value, SyncError> {
    let url = format!("{API}{path}");
    let builder = if method_patch { Request::patch(&url) } else { Request::post(&url) };
    let req = authed(builder, token)
        .json(body)
        .map_err(|e| SyncError::Network(e.to_string()))?;
    send(req).await
}

/// Looks for an existing gist containing the data file.
async fn find_gist(token: &str) -> Result<Option<String>, SyncError> {
    for page in 1..=10 {
        let list = get(&format!("/gists?per_page=100&page={page}"), token).await?;
        let Some(items) = list.as_array() else { break };
        if let Some(g) = items.iter().find(|g| g["files"].get(FILE).is_some()) {
            return Ok(g["id"].as_str().map(str::to_string));
        }
        if items.len() < 100 {
            break;
        }
    }
    Ok(None)
}

async fn read_gist(token: &str, id: &str) -> Result<Option<AppData>, SyncError> {
    let gist = get(&format!("/gists/{id}"), token).await?;
    let file = &gist["files"][FILE];
    if file.is_null() {
        return Ok(None);
    }
    let content = if file["truncated"].as_bool() == Some(true) {
        let url = file["raw_url"].as_str().unwrap_or_default();
        let res = Request::get(url)
            .send()
            .await
            .map_err(|e| SyncError::Network(e.to_string()))?;
        res.text().await.map_err(|e| SyncError::Network(e.to_string()))?
    } else {
        file["content"].as_str().unwrap_or_default().to_string()
    };
    if content.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str::<AppData>(&content)
        .map(|d| Some(d.normalized()))
        .map_err(|e| SyncError::BadRemote(e.to_string()))
}

/// Pulls the gist, merges it into local data and pushes the result if needed.
pub async fn sync_once(store: Store) -> Result<SyncResult, SyncError> {
    let conf = store.sync.get_untracked();
    let token = conf.token.trim().to_string();
    if token.is_empty() {
        return Err(SyncError::NoToken);
    }

    let mut id = conf.gist_id.clone();
    let mut remote = None;
    if !id.is_empty() {
        match read_gist(&token, &id).await {
            Ok(r) => remote = r,
            Err(SyncError::Http(404, _)) => id.clear(),
            Err(e) => return Err(e),
        }
    }
    if id.is_empty()
        && let Some(found) = find_gist(&token).await?
    {
        remote = read_gist(&token, &found).await?;
        id = found;
    }

    let pulled = remote.as_ref().map_or(0, |r| store.merge_remote(r));
    let local = store.snapshot();
    if remote.as_ref() != Some(&local) {
        let content = serde_json::to_string_pretty(&local).map_err(|e| SyncError::BadRemote(e.to_string()))?;
        let files = json!({ FILE: { "content": content } });
        if id.is_empty() {
            let body = json!({ "description": "Fuel Log data", "public": false, "files": files });
            let created = write(false, "/gists", &token, &body).await?;
            id = created["id"].as_str().unwrap_or_default().to_string();
        } else {
            write(true, &format!("/gists/{id}"), &token, &json!({ "files": files })).await?;
        }
    }

    let now = web::now_ms();
    store.update_sync(|c| {
        c.gist_id = id;
        c.last_sync = now;
    });
    Ok(SyncResult { pulled })
}
