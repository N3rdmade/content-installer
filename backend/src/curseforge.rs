use axum::{extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use shared::{GetState, models::user::GetPermissionManager};

use crate::settings::ContentInstallerSettings;

const CF_BASE: &str = "https://api.curseforge.com";
const CF_MINECRAFT_GAME_ID: u32 = 432;

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

async fn get_api_key(state: &GetState) -> Result<String, (StatusCode, String)> {
    let settings = state
        .settings
        .get()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Settings error: {e}")))?;
    let ext = settings
        .find_extension_settings::<ContentInstallerSettings>()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Extension settings not found"))?;
    if ext.curseforge_api_key.is_empty() {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "CurseForge API key not configured"));
    }
    Ok(ext.curseforge_api_key.to_string())
}

/// One client for every CurseForge call. The old per-request `Client::new()` paid a full
/// TLS handshake each time and defeated connection pooling — from CurseForge's side that
/// looks burstier than the same traffic pooled. The panel's core clients all set a
/// User-Agent; this one now does too (reqwest sends none by default, which is both rude
/// and the kind of thing CDN bot rules key on).
fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!(
                "gg.ir77.contentinstaller/",
                env!("CARGO_PKG_VERSION"),
                " (github.com/Regrave/content-installer)"
            ))
            .build()
            .expect("static client config cannot fail")
    })
}

/// How long a cached body is served without asking CurseForge again.
pub const TTL_SEARCH: std::time::Duration = std::time::Duration::from_secs(60);
pub const TTL_FILES: std::time::Duration = std::time::Duration::from_secs(60);
pub const TTL_DESCRIPTION: std::time::Duration = std::time::Duration::from_secs(3600);
pub const TTL_CATEGORIES: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Everything here is public mod metadata keyed by full URL, so one panel-wide map is
/// safe. Entries are kept past their TTL on purpose: when CurseForge is down (2026-07-27:
/// valid keys 403ing, /categories 504ing for hours) a stale listing beats an error page.
/// Capped by evicting the oldest insert; at ~90 KB per search page that bounds worst-case
/// memory around 12 MB.
const CACHE_MAX_ENTRIES: usize = 128;

fn cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, String)>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, String)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn cache_get(url: &str, max_age: Option<std::time::Duration>) -> Option<String> {
    let map = cache().lock().ok()?;
    let (inserted, body) = map.get(url)?;
    match max_age {
        Some(ttl) if inserted.elapsed() > ttl => None,
        _ => Some(body.clone()),
    }
}

fn cache_put(url: &str, body: &str) {
    let Ok(mut map) = cache().lock() else { return };
    if map.len() >= CACHE_MAX_ENTRIES && !map.contains_key(url) {
        if let Some(oldest) = map
            .iter()
            .min_by_key(|(_, (t, _))| *t)
            .map(|(k, _)| k.clone())
        {
            map.remove(&oldest);
        }
    }
    map.insert(url.to_string(), (std::time::Instant::now(), body.to_string()));
}

/// Authenticated GET against the CurseForge API, returning the raw response body.
///
/// Serves from cache within `ttl`; on any upstream failure falls back to a stale cached
/// body when one exists, so a CurseForge incident degrades to slightly-old data instead
/// of an error page. A 429 with a short Retry-After is retried once before giving up.
///
/// Upstream 401/403 means the configured key is wrong — a user-fixable setting, not a
/// gateway failure — so it does not collapse into a 502 the way it used to (#22). 502 is
/// kept for genuine transport errors so "bad gateway" still means "could not reach CurseForge".
async fn cf_get(
    url: &str,
    api_key: &str,
    ttl: std::time::Duration,
) -> Result<String, (StatusCode, String)> {
    if let Some(fresh) = cache_get(url, Some(ttl)) {
        return Ok(fresh);
    }
    match cf_fetch(url, api_key).await {
        Ok(body) => {
            cache_put(url, &body);
            Ok(body)
        }
        // Stale beats broken for public listings. The error still surfaces on queries that
        // were never cached, so a genuinely bad key does not stay hidden for long.
        Err(e) => cache_get(url, None).ok_or(e),
    }
}

async fn cf_fetch(url: &str, api_key: &str) -> Result<String, (StatusCode, String)> {
    let mut resp = http_client()
        .get(url)
        .header("x-api-key", api_key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("CurseForge request failed: {e}")))?;

    // One polite retry when rate limited and the advertised wait is short. Capped at 3s —
    // the user is holding an open HTTP request, not running a batch job.
    if resp.status() == StatusCode::TOO_MANY_REQUESTS {
        let wait = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(1)
            .min(3);
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        resp = http_client()
            .get(url)
            .header("x-api-key", api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("CurseForge request failed: {e}")))?;
    }

    let status = resp.status();
    // Who actually answered. CurseForge's own API is served by Kestrel; anything else on a
    // 401/403 means a proxy or CDN edge replied instead of them, which is the difference
    // between "your key is wrong" and "your server never reached CurseForge" (#22).
    let via = resp
        .headers()
        .get(reqwest::header::SERVER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unidentified")
        .to_string();
    let body = resp
        .text()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Failed to read response: {e}")))?;

    if status.is_success() {
        return Ok(body);
    }

    Err(match status {
        // Keep the upstream body. A 401/403 usually comes from CurseForge itself, but an
        // intermediary proxy can produce one too, and only the body tells them apart —
        // CurseForge answers `Forbidden: API Key missing or invalid` in plain text.
        // Dropping it for a canned message cost a full round-trip with a reporter (#22).
        // Observed 2026-07-27: during a CurseForge incident this exact 403 came back for a key
        // that had worked minutes earlier and was never changed, while /categories and /games
        // returned 504 from their load balancer. CurseForge reuses this one message for a bad
        // key, for rate limiting, and for their own outages, so it cannot be reported as a key
        // problem — telling users to re-paste a valid key sends them in circles (#22).
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if via == "Kestrel" => err(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "CurseForge refused the request ({status}). They send this same message for an \
                 invalid key, for rate limiting, and during their own outages. If browsing \
                 worked recently and the key has not changed, it is very likely their end — \
                 wait and retry before changing anything. Otherwise check the key under \
                 Admin -> Content Installer: exactly 60 characters, starting `$2a$`. \
                 Upstream said: {body}"
            ),
        ),
        // Not Kestrel, so CurseForge's application never evaluated the key. Either their own
        // edge (CloudFront / awselb) answered during an incident, or something local — a
        // proxy env var, egress filtering — intercepted the request. Both are possible and
        // the wording must not pick one: during the 2026-07-27 outage their edge produced
        // exactly this shape, and "check your proxy" would have been wrong advice.
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => err(
            StatusCode::BAD_GATEWAY,
            format!(
                "{status} from `{via}` — CurseForge's application never evaluated the key \
                 (their API answers as `Kestrel`). Either CurseForge's edge is having an \
                 incident (wait and retry), or something between this panel and \
                 api.curseforge.com intercepted the request (check \
                 HTTP_PROXY/HTTPS_PROXY/ALL_PROXY in the panel's environment and any egress \
                 filtering). Not an API key problem. Upstream said: {body}"
            ),
        ),
        StatusCode::TOO_MANY_REQUESTS => err(
            StatusCode::TOO_MANY_REQUESTS,
            "CurseForge rate limit reached (retried once already). Try again in a moment."
                .to_string(),
        ),
        // Their load balancer and CDN answer as `awselb/2.0` / `CloudFront`. A 5xx from those is
        // CurseForge's own infrastructure failing, never anything the panel operator can fix.
        _ if status.is_server_error() => err(
            StatusCode::BAD_GATEWAY,
            format!(
                "CurseForge is having problems: {status} from `{via}`. This is an outage on \
                 their side, not a panel or API key issue. Try again later."
            ),
        ),
        _ => err(
            StatusCode::BAD_GATEWAY,
            format!("CurseForge returned {status} (via `{via}`): {body}"),
        ),
    })
}

// ---- Search ----

#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(rename = "searchFilter")]
    search_filter: Option<String>,
    #[serde(rename = "classId")]
    class_id: Option<u32>,
    #[serde(rename = "gameVersion")]
    game_version: Option<String>,
    #[serde(rename = "modLoaderType")]
    mod_loader_type: Option<u32>,
    #[serde(rename = "sortField")]
    sort_field: Option<u32>,
    #[serde(rename = "sortOrder")]
    sort_order: Option<String>,
    #[serde(rename = "categoryIds")]
    category_ids: Option<String>,
    index: Option<u32>,
    #[serde(rename = "pageSize")]
    page_size: Option<u32>,
}

/// GET /content-installer/curseforge/search
pub async fn search(
    state: GetState,
    _permissions: GetPermissionManager,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let api_key = get_api_key(&state).await?;

    let mut url = format!("{CF_BASE}/v1/mods/search?gameId={CF_MINECRAFT_GAME_ID}");
    if let Some(ref q) = params.search_filter {
        url.push_str(&format!("&searchFilter={}", urlencoding::encode(q)));
    }
    if let Some(cid) = params.class_id {
        url.push_str(&format!("&classId={cid}"));
    }
    if let Some(ref gv) = params.game_version {
        url.push_str(&format!("&gameVersion={}", urlencoding::encode(gv)));
    }
    if let Some(mlt) = params.mod_loader_type {
        url.push_str(&format!("&modLoaderType={mlt}"));
    }
    if let Some(sf) = params.sort_field {
        url.push_str(&format!("&sortField={sf}"));
    }
    if let Some(ref so) = params.sort_order {
        url.push_str(&format!("&sortOrder={so}"));
    }
    if let Some(ref cids) = params.category_ids {
        // Comma-separated ids from the frontend; CF wants a stringified array,
        // e.g. categoryIds=[423,426]. Max 10 per CF docs.
        let ids: Vec<u32> = cids
            .split(',')
            .map(|s| s.trim().parse::<u32>())
            .collect::<Result<_, _>>()
            .map_err(|_| err(StatusCode::BAD_REQUEST, "Invalid categoryIds"))?;
        if ids.len() > 10 {
            return Err(err(StatusCode::BAD_REQUEST, "At most 10 categoryIds allowed"));
        }
        if !ids.is_empty() {
            let list = ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            url.push_str(&format!("&categoryIds={}", urlencoding::encode(&format!("[{list}]"))));
        }
    }
    url.push_str(&format!("&index={}", params.index.unwrap_or(0)));
    url.push_str(&format!("&pageSize={}", params.page_size.unwrap_or(20)));

    let body = cf_get(&url, &api_key, TTL_SEARCH).await?;

    Ok((
        StatusCode::OK,
        [("content-type", "application/json")],
        body,
    ))
}

// ---- Get categories ----

#[derive(Deserialize)]
pub struct CategoriesParams {
    #[serde(rename = "classId")]
    class_id: Option<u32>,
}

/// GET /content-installer/curseforge/categories
pub async fn categories(
    state: GetState,
    _permissions: GetPermissionManager,
    Query(params): Query<CategoriesParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let api_key = get_api_key(&state).await?;

    let mut url = format!("{CF_BASE}/v1/categories?gameId={CF_MINECRAFT_GAME_ID}");
    if let Some(cid) = params.class_id {
        url.push_str(&format!("&classId={cid}"));
    }

    let body = cf_get(&url, &api_key, TTL_CATEGORIES).await?;

    Ok((
        StatusCode::OK,
        [("content-type", "application/json")],
        body,
    ))
}

// ---- Get mod files ----

#[derive(Deserialize)]
pub struct FilesParams {
    #[serde(rename = "modId")]
    mod_id: u32,
    #[serde(rename = "gameVersion")]
    game_version: Option<String>,
    #[serde(rename = "modLoaderType")]
    mod_loader_type: Option<u32>,
    index: Option<u32>,
    #[serde(rename = "pageSize")]
    page_size: Option<u32>,
}

/// GET /content-installer/curseforge/files
pub async fn files(
    state: GetState,
    _permissions: GetPermissionManager,
    Query(params): Query<FilesParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let api_key = get_api_key(&state).await?;

    let mut url = format!("{CF_BASE}/v1/mods/{}/files?", params.mod_id);
    if let Some(ref gv) = params.game_version {
        url.push_str(&format!("gameVersion={}&", urlencoding::encode(gv)));
    }
    if let Some(mlt) = params.mod_loader_type {
        url.push_str(&format!("modLoaderType={mlt}&"));
    }
    url.push_str(&format!("index={}", params.index.unwrap_or(0)));
    url.push_str(&format!("&pageSize={}", params.page_size.unwrap_or(20)));

    let body = cf_get(&url, &api_key, TTL_FILES).await?;

    Ok((
        StatusCode::OK,
        [("content-type", "application/json")],
        body,
    ))
}

// ---- Get mod description ----

#[derive(Deserialize)]
pub struct DescriptionParams {
    #[serde(rename = "modId")]
    mod_id: u32,
}

/// GET /content-installer/curseforge/description
pub async fn description(
    state: GetState,
    _permissions: GetPermissionManager,
    Query(params): Query<DescriptionParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let api_key = get_api_key(&state).await?;

    let url = format!("{CF_BASE}/v1/mods/{}/description", params.mod_id);

    let body = cf_get(&url, &api_key, TTL_DESCRIPTION).await?;

    Ok((
        StatusCode::OK,
        [("content-type", "application/json")],
        body,
    ))
}

// ---- Check if configured ----

/// GET /content-installer/curseforge/status
pub async fn status(
    state: GetState,
    _permissions: GetPermissionManager,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let has_key = get_api_key(&state).await.is_ok();
    Ok(axum::Json(serde_json::json!({ "configured": has_key })))
}
