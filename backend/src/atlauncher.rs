use axum::{extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use shared::models::user::GetPermissionManager;

const API: &str = "https://api.atlauncher.com/v1";
const CDN: &str = "https://download.nodecdn.net/containers/atl/";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(40))
            .user_agent(UA)
            .build()
            .expect("static ATLauncher client config cannot fail")
    })
}

async fn get_json(url: &str) -> Result<Value, (StatusCode, String)> {
    let response = client()
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("ATLauncher request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(err(StatusCode::BAD_GATEWAY, format!("ATLauncher returned HTTP {status}")));
    }
    response
        .json::<Value>()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Invalid ATLauncher response: {e}")))
}

fn pack_image(safe_name: &str) -> Option<String> {
    let safe_name = safe_name.trim().to_ascii_lowercase();
    (!safe_name.is_empty()).then(|| format!("{CDN}launcher/images/{}.png", urlencoding::encode(&safe_name)))
}

fn normalized_versions(versions: &[Value]) -> Vec<Value> {
    versions.iter().filter_map(|version| {
        let number = version.get("version").and_then(Value::as_str)?;
        let mc = version.get("minecraft").and_then(Value::as_str);
        Some(json!({
            "id": number,
            "name": number,
            "versionNumber": number,
            "displayName": mc.map(|mc| format!("{number} — MC {mc}")).unwrap_or_else(|| number.to_string()),
            "gameVersion": mc,
            "loaders": [],
        }))
    }).collect()
}

fn normalize_pack(pack: &Value) -> Value {
    let versions = pack.get("versions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let latest = versions.first();
    let safe_name = pack.get("safeName").and_then(Value::as_str).unwrap_or("");
    json!({
        "provider": "atlauncher",
        "id": if safe_name.is_empty() { pack.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default() } else { safe_name.to_string() },
        "slug": safe_name,
        "name": pack.get("name").and_then(Value::as_str).unwrap_or("Unknown"),
        "summary": pack.get("description").and_then(Value::as_str).unwrap_or(""),
        "description": pack.get("description").and_then(Value::as_str).unwrap_or(""),
        "downloadCount": 0,
        "iconUrl": pack_image(safe_name),
        "author": "ATLauncher",
        "gameVersions": latest.and_then(|v| v.get("minecraft")).and_then(Value::as_str),
        "loaders": [],
        "latestFileId": latest.and_then(|v| v.get("version")).and_then(Value::as_str),
        "availableVersions": normalized_versions(versions),
        "websiteUrl": pack.get("websiteURL").and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| format!("https://atlauncher.com/pack/{}", urlencoding::encode(safe_name))),
        "gallery": [],
    })
}

async fn all_packs() -> Result<Vec<Value>, (StatusCode, String)> {
    let data = get_json(&format!("{API}/packs/full/all")).await?;
    Ok(data.get("data").and_then(Value::as_array).cloned().unwrap_or_default()
        .into_iter()
        .filter(|pack| pack.get("type").and_then(Value::as_str) == Some("public")
            && pack.get("versions").and_then(Value::as_array).is_some_and(|v| !v.is_empty()))
        .collect())
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
}
fn default_page_size() -> usize { 20 }

pub async fn search(
    _permissions: GetPermissionManager,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut packs = all_packs().await?;
    let query = params.query.trim().to_ascii_lowercase();
    if !query.is_empty() {
        packs.retain(|pack| pack.get("name").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase().contains(&query));
    }
    if let Some(version) = params.game_version.as_deref() {
        packs.retain(|pack| pack.get("versions").and_then(Value::as_array).is_some_and(|versions| {
            versions.iter().any(|v| v.get("minecraft").and_then(Value::as_str) == Some(version))
        }));
    }
    packs.sort_by_key(|pack| std::cmp::Reverse(pack.get("id").and_then(Value::as_i64).unwrap_or(0)));
    let page_size = params.page_size.clamp(1, 40);
    let start = params.page * page_size;
    let items = packs.into_iter().skip(start).take(page_size).map(|pack| normalize_pack(&pack)).collect::<Vec<_>>();
    Ok(axum::Json(json!({"data": items, "hasMore": items.len() == page_size})))
}

#[derive(Deserialize)]
pub struct VersionsParams { safe_name: String }

pub async fn versions(
    _permissions: GetPermissionManager,
    Query(params): Query<VersionsParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let packs = all_packs().await?;
    let pack = packs.into_iter().find(|pack| {
        pack.get("safeName").and_then(Value::as_str) == Some(params.safe_name.as_str())
            || pack.get("id").map(|v| v.to_string().trim_matches('"').to_string()) == Some(params.safe_name.clone())
    }).ok_or_else(|| err(StatusCode::NOT_FOUND, "ATLauncher pack not found"))?;
    let versions = pack.get("versions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    Ok(axum::Json(json!({"data": normalized_versions(versions)})))
}

fn encode_path(path: &str) -> String {
    path.split('/').map(|segment| urlencoding::encode(segment).into_owned()).collect::<Vec<_>>().join("/")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallFile {
    pub name: String,
    pub dir: String,
    pub url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallManifest {
    pub files: Vec<InstallFile>,
    pub configs_url: Option<String>,
    pub loader: Option<String>,
    pub minecraft: Option<String>,
    pub loader_version: Option<String>,
    pub java: Option<u8>,
}

pub async fn install_manifest(safe_name: &str, version: &str) -> Result<InstallManifest, (StatusCode, String)> {
    let base = format!("{CDN}packs/{}/versions/{}/", encode_path(safe_name), encode_path(version));
    let data = get_json(&format!("{base}Configs.json")).await?;
    let loader_data = data.get("loader").cloned().unwrap_or(Value::Null);
    let loader = loader_data.get("type").and_then(Value::as_str).map(|v| v.to_ascii_lowercase()).filter(|v| matches!(v.as_str(), "forge" | "neoforge" | "fabric" | "quilt"));
    let mut files = Vec::new();

    for item in data.get("mods").and_then(Value::as_array).cloned().unwrap_or_default() {
        if item.get("server").and_then(Value::as_bool) == Some(false) { continue; }
        if item.get("library").and_then(Value::as_bool) == Some(true) { continue; }
        if item.get("type").and_then(Value::as_str).unwrap_or("mods") != "mods" { continue; }
        if item.get("download").and_then(Value::as_str) == Some("browser") { continue; }
        let Some(raw_url) = item.get("url").and_then(Value::as_str) else { continue };
        let download = item.get("download").and_then(Value::as_str).unwrap_or("server");
        let url = if download == "direct" { raw_url.to_string() } else { format!("{CDN}{}", encode_path(raw_url)) };
        let name = item.get("file").and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| raw_url.rsplit('/').next().unwrap_or("mod.jar").to_string());
        files.push(InstallFile { name, dir: "mods".into(), url });
    }

    Ok(InstallManifest {
        files,
        configs_url: data.get("configs").and_then(Value::as_bool).filter(|v| *v).map(|_| format!("{base}Configs.zip")),
        loader,
        minecraft: data.get("minecraft").and_then(Value::as_str).map(ToString::to_string)
            .or_else(|| loader_data.pointer("/metadata/minecraft").and_then(Value::as_str).map(ToString::to_string)),
        loader_version: loader_data.pointer("/metadata/version").and_then(Value::as_str).map(ToString::to_string)
            .or_else(|| loader_data.get("version").and_then(Value::as_str).map(ToString::to_string)),
        java: data.pointer("/java/min").and_then(Value::as_u64).and_then(|v| u8::try_from(v).ok()),
    })
}
