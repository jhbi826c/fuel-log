use std::collections::BTreeMap;

use gloo_net::http::{Request, RequestBuilder, Response};
use leptos::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};
use web_sys::RequestCache;

use crate::model::{AppData, Record, Vehicle};
use crate::store::Store;
use crate::web;

const API: &str = "https://api.github.com";
pub const DEFAULT_REPO: &str = "fuel-log-data";
const VEHICLES: &str = "vehicles.json";
const RECORDS: &str = "records/";
const README: &str = "# Fuel Log data\n\nSynced by Fuel Log. Vehicles are in `vehicles.json`, fill-ups in \
                      `records/<year>.json`, one per line.\n";

#[derive(Debug, Clone)]
pub enum SyncError {
    NoToken,
    Unauthorized,
    Forbidden(String),
    NoRepo(String),
    Conflict,
    Http(u16, String),
    Network(String),
    BadRemote(String),
}

#[derive(Debug, Clone, Copy)]
pub struct SyncResult {
    pub pulled: usize,
}

fn record_file(r: &Record) -> String {
    let year = r
        .date
        .get(..4)
        .filter(|y| y.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or("undated");
    format!("{RECORDS}{year}.json")
}

fn is_data_file(path: &str) -> bool {
    path == VEHICLES || (path.starts_with(RECORDS) && path.ends_with(".json"))
}

/// One item per line for readable diffs.
fn json_lines<T: Serialize>(items: &[&T]) -> String {
    let lines: Vec<String> = items.iter().filter_map(|i| serde_json::to_string(i).ok()).collect();
    if lines.is_empty() {
        "[]\n".into()
    } else {
        format!("[\n{}\n]\n", lines.join(",\n"))
    }
}

fn to_files(data: &AppData) -> BTreeMap<String, String> {
    let mut years: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in &data.records {
        years.entry(record_file(r)).or_default().push(r);
    }
    let mut files: BTreeMap<String, String> = years
        .into_iter()
        .map(|(path, mut records)| {
            records.sort_by(|a, b| (&a.date, &a.id).cmp(&(&b.date, &b.id)));
            (path, json_lines(&records))
        })
        .collect();
    let mut vehicles: Vec<&Vehicle> = data.vehicles.iter().collect();
    vehicles.sort_by(|a, b| a.id.cmp(&b.id));
    files.insert(VEHICLES.into(), json_lines(&vehicles));
    files
}

fn read_file(data: &mut AppData, path: &str, content: &str) -> Result<(), SyncError> {
    let bad = |e: serde_json::Error| SyncError::BadRemote(format!("{path}: {e}"));
    if path == VEHICLES {
        data.vehicles.extend(serde_json::from_str::<Vec<Vehicle>>(content).map_err(bad)?);
    } else {
        data.records.extend(serde_json::from_str::<Vec<Record>>(content).map_err(bad)?);
    }
    Ok(())
}

fn blob_sha(content: &str) -> String {
    let mut hash = sha1_smol::Sha1::new();
    hash.update(format!("blob {}\0", content.len()).as_bytes());
    hash.update(content.as_bytes());
    hash.digest().to_string()
}

fn to_download(local: &BTreeMap<String, String>, remote: &BTreeMap<String, String>) -> Vec<String> {
    remote
        .iter()
        .filter(|(path, sha)| is_data_file(path) && local.get(*path).is_none_or(|c| blob_sha(c) != **sha))
        .map(|(path, _)| path.clone())
        .collect()
}

fn changes(files: &BTreeMap<String, String>, remote: &BTreeMap<String, String>) -> Vec<(String, Option<String>)> {
    let mut changes: Vec<(String, Option<String>)> = files
        .iter()
        .filter(|(path, content)| remote.get(*path) != Some(&blob_sha(content)))
        .map(|(path, content)| (path.clone(), Some(content.clone())))
        .collect();
    changes.extend(
        remote
            .keys()
            .filter(|path| is_data_file(path) && !files.contains_key(*path))
            .map(|path| (path.clone(), None)),
    );
    changes
}

pub async fn sync_once(store: Store) -> Result<SyncResult, SyncError> {
    let conf = store.sync.get_untracked();
    let token = conf.token.trim().to_string();
    if token.is_empty() {
        return Err(SyncError::NoToken);
    }
    let repo = open_repo(&token, &conf.repo).await?;
    let mut pulled = 0;
    for _ in 0..4 {
        let Some((parent, tree)) = head(&token, &repo).await? else {
            init(&token, &repo).await?;
            continue;
        };
        let remote = remote_files(&token, &repo, &tree).await?;
        let mut incoming = AppData::default();
        for path in to_download(&to_files(&store.snapshot()), &remote) {
            read_file(&mut incoming, &path, &blob(&token, &repo, &remote[&path]).await?)?;
        }
        pulled += store.merge_remote(&incoming);
        let changes = changes(&to_files(&store.snapshot()), &remote);
        if changes.is_empty() || commit(&token, &repo, &parent, &tree, &changes).await? {
            let now = web::now_ms();
            store.update_sync(|c| {
                c.repo = repo.name;
                c.last_sync = now;
            });
            return Ok(SyncResult { pulled });
        }
    }
    Err(SyncError::Conflict)
}

struct Repo {
    /// `owner/name`
    name: String,
    branch: String,
}

fn api(method: fn(&str) -> RequestBuilder, path: &str, token: &str) -> RequestBuilder {
    method(&format!("{API}{path}"))
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", &format!("Bearer {token}"))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .cache(RequestCache::NoStore)
}

fn network(e: gloo_net::Error) -> SyncError {
    SyncError::Network(e.to_string())
}

async fn fetch(req: Result<Request, gloo_net::Error>) -> Result<Response, SyncError> {
    let res = req.map_err(network)?.send().await.map_err(network)?;
    let status = res.status();
    if res.ok() {
        return Ok(res);
    }
    let msg = res
        .json::<Value>()
        .await
        .ok()
        .and_then(|v| v["message"].as_str().map(str::to_string))
        .unwrap_or_default();
    Err(match status {
        401 => SyncError::Unauthorized,
        403 => SyncError::Forbidden(msg),
        code => SyncError::Http(code, msg),
    })
}

async fn send(req: Result<Request, gloo_net::Error>) -> Result<Value, SyncError> {
    fetch(req).await?.json().await.map_err(network)
}

async fn get(token: &str, path: &str) -> Result<Value, SyncError> {
    send(api(Request::get, path, token).build()).await
}

async fn post(token: &str, path: &str, body: &Value) -> Result<Value, SyncError> {
    send(api(Request::post, path, token).json(body)).await
}

async fn login(token: &str) -> Result<String, SyncError> {
    let user = get(token, "/user").await?;
    Ok(user["login"].as_str().unwrap_or_default().to_string())
}

async fn open_repo(token: &str, repo: &str) -> Result<Repo, SyncError> {
    let repo = repo.trim().trim_matches('/');
    let mut user = None;
    let (owner, name) = match repo.split_once('/') {
        Some((owner, name)) => (owner.to_string(), name.to_string()),
        None => {
            let login = login(token).await?;
            user = Some(login.clone());
            (login, if repo.is_empty() { DEFAULT_REPO } else { repo }.to_string())
        }
    };
    let missing = || SyncError::NoRepo(format!("{owner}/{name}"));
    let info = match get(token, &format!("/repos/{owner}/{name}")).await {
        Err(SyncError::Http(404, _)) => {
            let user = match user {
                Some(user) => user,
                None => login(token).await?,
            };
            if !user.eq_ignore_ascii_case(&owner) {
                return Err(missing());
            }
            let body = json!({ "name": name, "private": true, "auto_init": true, "description": "Fuel Log data" });
            match post(token, "/user/repos", &body).await {
                // Name taken by a repo the token can't see.
                Err(SyncError::Http(422, _)) => return Err(missing()),
                // Tokens without the repo scope get 404.
                Err(SyncError::Http(404, m)) => return Err(SyncError::Forbidden(m)),
                other => other?,
            }
        }
        other => other?,
    };
    Ok(Repo {
        name: info["full_name"]
            .as_str()
            .map_or_else(|| format!("{owner}/{name}"), str::to_string),
        branch: info["default_branch"].as_str().unwrap_or("main").to_string(),
    })
}

async fn head(token: &str, repo: &Repo) -> Result<Option<(String, String)>, SyncError> {
    match get(token, &format!("/repos/{}/branches/{}", repo.name, repo.branch)).await {
        Ok(branch) => {
            let commit = &branch["commit"];
            let sha = |v: &Value| v.as_str().unwrap_or_default().to_string();
            Ok(Some((sha(&commit["sha"]), sha(&commit["commit"]["tree"]["sha"]))))
        }
        Err(SyncError::Http(404 | 409, _)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The Git Data API fails on empty repos.
async fn init(token: &str, repo: &Repo) -> Result<(), SyncError> {
    let content = web_sys::window().and_then(|w| w.btoa(README).ok()).unwrap_or_default();
    let body = json!({ "message": "Create Fuel Log data", "content": content });
    let path = format!("/repos/{}/contents/README.md", repo.name);
    send(api(Request::put, &path, token).json(&body)).await.map(|_| ())
}

async fn remote_files(token: &str, repo: &Repo, tree: &str) -> Result<BTreeMap<String, String>, SyncError> {
    let tree = get(token, &format!("/repos/{}/git/trees/{tree}?recursive=1", repo.name)).await?;
    Ok(tree["tree"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["type"] == "blob")
        .filter_map(|f| Some((f["path"].as_str()?.to_string(), f["sha"].as_str()?.to_string())))
        .collect())
}

async fn blob(token: &str, repo: &Repo, sha: &str) -> Result<String, SyncError> {
    let req = api(Request::get, &format!("/repos/{}/git/blobs/{sha}", repo.name), token)
        .header("Accept", "application/vnd.github.raw+json")
        .build();
    fetch(req).await?.text().await.map_err(network)
}

/// `false` if another device pushed first.
async fn commit(
    token: &str,
    repo: &Repo,
    parent: &str,
    base_tree: &str,
    changes: &[(String, Option<String>)],
) -> Result<bool, SyncError> {
    let items: Vec<Value> = changes
        .iter()
        .map(|(path, content)| match content {
            Some(content) => json!({ "path": path, "mode": "100644", "type": "blob", "content": content }),
            None => json!({ "path": path, "mode": "100644", "type": "blob", "sha": null }),
        })
        .collect();
    let path = format!("/repos/{}/git/trees", repo.name);
    let tree = post(token, &path, &json!({ "base_tree": base_tree, "tree": items })).await?;
    let tree = tree["sha"].as_str().unwrap_or_default();
    if tree == base_tree {
        return Ok(true);
    }
    let path = format!("/repos/{}/git/commits", repo.name);
    let body = json!({ "message": "Sync fuel log", "tree": tree, "parents": [parent] });
    let commit = post(token, &path, &body).await?;
    let body = json!({ "sha": commit["sha"], "force": false });
    let path = format!("/repos/{}/git/refs/heads/{}", repo.name, repo.branch);
    match send(api(Request::patch, &path, token).json(&body)).await {
        Ok(_) => Ok(true),
        Err(SyncError::Http(409 | 422, _)) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, date: &str, t: f64) -> Record {
        Record {
            id: id.into(),
            vehicle_id: "v".into(),
            date: date.into(),
            odometer: Some(t * 100.0),
            updated_at: t,
            ..Default::default()
        }
    }

    fn data() -> AppData {
        let mut d = AppData {
            vehicles: vec![Vehicle { id: "v".into(), name: "Car".into(), updated_at: 1.0, deleted: false, fuels: Vec::new() }],
            ..Default::default()
        };
        for (i, year) in ["2024", "2025", "2026"].iter().enumerate() {
            for m in 1..=6 {
                d.records.push(record(&format!("r{year}-{m}"), &format!("{year}-0{m}-01T10:00"), (i * 10 + m) as f64));
            }
        }
        d.normalized()
    }

    fn shas(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        files.iter().map(|(p, c)| (p.clone(), blob_sha(c))).collect()
    }

    fn read_all(files: &BTreeMap<String, String>, paths: &[String]) -> AppData {
        let mut d = AppData::default();
        for path in paths {
            read_file(&mut d, path, &files[path]).unwrap();
        }
        d
    }

    fn merged(a: &AppData, b: &AppData) -> AppData {
        let mut a = a.clone();
        a.merge(b);
        a.normalized()
    }

    #[test]
    fn blob_ids_match_git() {
        assert_eq!(blob_sha("hello\n"), "ce013625030ba8dba906f756967f9e9ca394464a");
    }

    #[test]
    fn records_are_split_by_year() {
        let files = to_files(&data());
        let paths: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(paths, ["records/2024.json", "records/2025.json", "records/2026.json", VEHICLES]);
        assert!(files.keys().all(|p| is_data_file(p)));
        assert_eq!(files.values().filter(|c| c.contains("\"r2025-3\"")).count(), 1);
        assert_eq!(record_file(&Record { deleted: true, ..Default::default() }), "records/undated.json");
    }

    #[test]
    fn files_round_trip() {
        let d = data();
        let files = to_files(&d);
        let paths: Vec<String> = files.keys().cloned().collect();
        assert_eq!(read_all(&files, &paths).normalized(), d);
        assert_eq!(to_files(&read_all(&files, &paths).normalized()), files);
    }

    #[test]
    fn only_changed_files_are_pulled_and_pushed() {
        let local = data();
        let mut other = local.clone();
        let r = other.records.iter_mut().find(|r| r.id == "r2026-2").unwrap();
        r.station = "Aral".into();
        r.updated_at = 99.0;
        let remote = to_files(&other);

        assert!(to_download(&to_files(&local), &shas(&to_files(&local))).is_empty());
        let paths = to_download(&to_files(&local), &shas(&remote));
        assert_eq!(paths, ["records/2026.json"]);
        let after = merged(&local, &read_all(&remote, &paths));
        assert_eq!(after, other);
        assert!(changes(&to_files(&after), &shas(&remote)).is_empty());

        let mut deleted = after.clone();
        deleted.records.iter_mut().find(|r| r.id == "r2024-5").unwrap().deleted = true;
        let changed: Vec<String> = changes(&to_files(&deleted), &shas(&remote)).into_iter().map(|(p, _)| p).collect();
        assert_eq!(changed, ["records/2024.json"]);
    }

    #[test]
    fn moved_records_leave_and_emptied_files_are_removed() {
        let mut d = AppData::default();
        d.records.push(record("x", "2025-12-31T23:00", 1.0));
        let remote = shas(&to_files(&d));
        d.records[0].date = "2026-01-01T00:10".into();
        d.records[0].updated_at = 2.0;
        let changes = changes(&to_files(&d), &remote);
        assert_eq!(
            changes.iter().map(|(p, c)| (p.as_str(), c.is_some())).collect::<Vec<_>>(),
            [("records/2026.json", true), ("records/2025.json", false)]
        );
    }

    #[test]
    fn unreadable_remote_file_is_an_error() {
        let mut d = AppData::default();
        assert!(matches!(read_file(&mut d, "records/2026.json", "{"), Err(SyncError::BadRemote(_))));
        assert!(d.records.is_empty());
    }
}
