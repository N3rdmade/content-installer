use axum::{extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use shared::models::user::GetPermissionManager;

const BASE: &str = "https://api.modpacks.ch/public";

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("N3rdmade/content-installer/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("static FTB client config cannot fail")
    })
}

async fn get_json(url: &str) -> Result<Value, (StatusCode, String)> {
    let response = client()
        .get(url)
        .send()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("FTB request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(err(StatusCode::BAD_GATEWAY, format!("FTB returned HTTP {status}")));
    }
    response
        .json::<Value>()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Invalid FTB response: {e}")))
}

fn target_meta(targets: &[Value]) -> (Option<String>, Option<String>, Option<String>, Option<u8>) {
    let mut loader = None;
    let mut mc = None;
    let mut loader_version = None;
    let mut java = None;

    for target in targets {
        let kind = target.get("type").and_then(Value::as_str).unwrap_or("");
        let name = target.get("name").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
        let version = target.get("version").and_then(Value::as_str).map(ToString::to_string);
        match (kind, name.as_str()) {
            ("modloader", "forge" | "neoforge" | "fabric" | "quilt") => {
                loader = Some(name);
                loader_version = version;
            }
            ("game", "minecraft") => mc = version,
            ("runtime", "java") => {
                java = version
                    .as_deref()
                    .and_then(|v| v.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|v| v.parse::<u8>().ok());
            }
            _ => {}
        }
    }

    (loader, mc, loader_version, java)
}

fn square_art(art: &[Value]) -> Option<String> {
    art.iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("square"))
        .and_then(|item| item.get("url").and_then(Value::as_str))
        .or_else(|| art.first().and_then(|item| item.get("url").and_then(Value::as_str)))
        .map(ToString::to_string)
}

fn normalize_pack(pack: &Value) -> Value {
    let versions = pack.get("versions").and_then(Value::as_array).cloned().unwrap_or_default();
    let latest = versions.first().cloned().unwrap_or(Value::Null);
    let targets = latest.get("targets").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let (loader, mc, _, _) = target_meta(targets);
    let art = pack.get("art").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let gallery: Vec<Value> = art
        .iter()
        .filter_map(|item| item.get("url").and_then(Value::as_str))
        .map(|url| json!({"url": url, "thumbnailUrl": url}))
        .collect();

    json!({
        "provider": "ftb",
        "id": pack.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default(),
        "slug": pack.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default(),
        "name": pack.get("name").and_then(Value::as_str).unwrap_or("Unknown"),
        "summary": pack.get("synopsis").and_then(Value::as_str).unwrap_or(""),
        "description": pack.get("description").and_then(Value::as_str).or_else(|| pack.get("synopsis").and_then(Value::as_str)).unwrap_or(""),
        "downloadCount": pack.get("installs").and_then(Value::as_u64).unwrap_or(0),
        "iconUrl": square_art(art),
        "author": pack.get("authors").and_then(Value::as_array).and_then(|a| a.first()).and_then(|a| a.get("name")).and_then(Value::as_str).unwrap_or("FTB"),
        "gameVersions": mc,
        "loaders": loader.clone().map(|l| vec![l]).unwrap_or_default(),
        "latestFileId": latest.get("id").map(|v| v.to_string().trim_matches('"').to_string()),
        "websiteUrl": format!("https://www.feed-the-beast.com/modpacks/{}", pack.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default()),
        "gallery": gallery,
    })
}

#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    query: String,
    #[serde(default)]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    game_version: Option<String>,
    loaders: Option<String>,
}

fn default_page_size() -> usize { 20 }

pub async fn search(
    _permissions: GetPermissionManager,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let page_size = params.page_size.clamp(1, 40);
    let requested_loaders: Vec<String> = params.loaders
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .collect();
    let needs_filter = params.game_version.is_some() || !requested_loaders.is_empty();
    let desired = (params.page + 1) * page_size;
    let fetch_limit = if needs_filter { (desired * 3).clamp(page_size * 3, 160) } else { desired.max(page_size) };

    let endpoint = if params.query.trim().is_empty() {
        format!("{BASE}/modpack/popular/installs/{fetch_limit}")
    } else {
        format!("{BASE}/modpack/search/{fetch_limit}?term={}", urlencoding::encode(params.query.trim()))
    };
    let ids = get_json(&endpoint).await?
        .get("packs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut packs = Vec::new();
    for id in ids {
        let Some(id) = id.as_u64() else { continue };
        if let Ok(pack) = get_json(&format!("{BASE}/modpack/{id}")).await {
            let normalized = normalize_pack(&pack);
            let version_ok = params.game_version.as_deref().is_none_or(|wanted| {
                normalized.get("gameVersions").and_then(Value::as_str) == Some(wanted)
            });
            let loader_ok = requested_loaders.is_empty() || normalized
                .get("loaders")
                .and_then(Value::as_array)
                .is_some_and(|values| values.iter().filter_map(Value::as_str).any(|v| requested_loaders.iter().any(|wanted| wanted == &v.to_ascii_lowercase())));
            if version_ok && loader_ok {
                packs.push(normalized);
            }
        }
    }

    let start = params.page * page_size;
    let items = packs.into_iter().skip(start).take(page_size).collect::<Vec<_>>();
    Ok(axum::Json(json!({"data": items, "hasMore": items.len() == page_size})))
}

#[derive(Deserialize)]
pub struct VersionsParams { pack_id: u64 }

pub async fn versions(
    _permissions: GetPermissionManager,
    Query(params): Query<VersionsParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let pack = get_json(&format!("{BASE}/modpack/{}", params.pack_id)).await?;
    let mut versions = pack.get("versions").and_then(Value::as_array).cloned().unwrap_or_default();
    versions.sort_by_key(|v| std::cmp::Reverse(v.get("updated").and_then(Value::as_i64).unwrap_or(0)));
    let out: Vec<Value> = versions.into_iter().filter_map(|version| {
        let id = version.get("id")?.clone();
        let targets = version.get("targets").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        let (loader, mc, loader_version, java) = target_meta(targets);
        Some(json!({
            "id": id,
            "name": version.get("name").and_then(Value::as_str).unwrap_or("Version"),
            "versionNumber": version.get("name").and_then(Value::as_str).unwrap_or(""),
            "displayName": version.get("name").and_then(Value::as_str).unwrap_or("Version"),
            "loader": loader,
            "loaderVersion": loader_version,
            "gameVersion": mc,
            "java": java,
        }))
    }).collect();
    Ok(axum::Json(json!({"data": out})))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallFile {
    pub name: String,
    pub dir: String,
    pub url: Option<String>,
    pub cf_project: Option<u64>,
    pub cf_file: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallManifest {
    pub files: Vec<InstallFile>,
    pub loader: Option<String>,
    pub minecraft: Option<String>,
    pub loader_version: Option<String>,
    pub java: Option<u8>,
}

pub async fn install_manifest(pack_id: u64, version_id: u64) -> Result<InstallManifest, (StatusCode, String)> {
    let data = get_json(&format!("{BASE}/modpack/{pack_id}/{version_id}")).await?;
    let targets = data.get("targets").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let (loader, minecraft, loader_version, java) = target_meta(targets);
    let mut files = Vec::new();

    for file in data.get("files").and_then(Value::as_array).cloned().unwrap_or_default() {
        if file.get("clientonly").and_then(Value::as_bool).unwrap_or(false) { continue; }
        let Some(name) = file.get("name").and_then(Value::as_str) else { continue };
        let dir = file.get("path").and_then(Value::as_str).unwrap_or("")
            .replace('\\', "/").trim_matches('/').to_string();
        files.push(InstallFile {
            name: name.to_string(),
            dir,
            url: file.get("url").and_then(Value::as_str).map(ToString::to_string),
            cf_project: file.pointer("/curseforge/project").and_then(Value::as_u64),
            cf_file: file.pointer("/curseforge/file").and_then(Value::as_u64),
        });
    }

    Ok(InstallManifest { files, loader, minecraft, loader_version, java })
}
