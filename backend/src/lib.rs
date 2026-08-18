mod curseforge;
mod install_script;
mod settings;

use axum::{extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use shared::{
    GetState,
    extensions::{Extension, ExtensionRouteBuilder},
    models::{
        server::GetServer,
        user::GetPermissionManager,
    },
    State,
};
use std::sync::Arc;

/// CurseForge API keys are bcrypt hashes and always this long.
const CF_API_KEY_LEN: usize = 60;

#[derive(Default)]
pub struct ExtensionStruct;

#[async_trait::async_trait]
impl Extension for ExtensionStruct {
    async fn initialize(&mut self, _state: State) {}

    async fn settings_deserializer(
        &self,
        _state: State,
    ) -> shared::extensions::settings::ExtensionSettingsDeserializer {
        Arc::new(settings::ContentInstallerSettingsDeserializer)
    }

    async fn initialize_router(
        &mut self,
        _state: State,
        builder: ExtensionRouteBuilder,
    ) -> ExtensionRouteBuilder {
        builder
            .add_client_server_api_router(|router| {
                router
                    .route(
                        "/content-installer/install",
                        axum::routing::post(install_content),
                    )
                    .route(
                        "/content-installer/install/status",
                        axum::routing::get(install_status),
                    )
                    .route(
                        "/content-installer/remove",
                        axum::routing::post(remove_content),
                    )
                    .route(
                        "/content-installer/curseforge/search",
                        axum::routing::get(curseforge::search),
                    )
                    .route(
                        "/content-installer/curseforge/files",
                        axum::routing::get(curseforge::files),
                    )
                    .route(
                        "/content-installer/curseforge/status",
                        axum::routing::get(curseforge::status),
                    )
                    .route(
                        "/content-installer/curseforge/categories",
                        axum::routing::get(curseforge::categories),
                    )
                    .route(
                        "/content-installer/curseforge/description",
                        axum::routing::get(curseforge::description),
                    )
                    .route(
                        "/content-installer/modpack/install",
                        axum::routing::post(modpack_install),
                    )
                    .route(
                        "/content-installer/modpack/cf-install",
                        axum::routing::post(cf_modpack_install),
                    )
            })
            .add_admin_api_router(|router| {
                router
                    .route(
                        "/content-installer/settings",
                        axum::routing::get(admin_get_settings),
                    )
                    .route(
                        "/content-installer/settings",
                        axum::routing::put(admin_put_settings),
                    )
            })
    }
}

// ---- Admin settings endpoints ----

async fn admin_get_settings(
    state: GetState,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let settings = state
        .settings
        .get()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    let ext = settings
        .find_extension_settings::<settings::ContentInstallerSettings>()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Settings not found"))?;
    // Mask the API key for display
    let masked = if ext.curseforge_api_key.is_empty() {
        String::new()
    } else {
        let key = &ext.curseforge_api_key;
        if key.len() > 8 {
            format!("{}...{}", &key[..4], &key[key.len()-4..])
        } else {
            "*".repeat(key.len())
        }
    };
    // CurseForge keys are bcrypt strings: exactly 60 chars, `$2a$10$...`, no whitespace.
    // The mask above only shows first-4 and last-4, so a key that lost or gained characters
    // in the middle looks identical to a good one — which is the failure mode in #22, where
    // re-pasting from the same bad source reproduced it every time. Length is the signal the
    // mask cannot carry, so report it and let the admin UI compare against 60.
    let key_len = ext.curseforge_api_key.chars().count();
    let malformed = !ext.curseforge_api_key.is_empty()
        && (!ext.curseforge_api_key.starts_with("$2a$")
            || key_len != CF_API_KEY_LEN
            || ext.curseforge_api_key.chars().any(|c| c.is_whitespace() || c.is_control()));
    Ok(axum::Json(serde_json::json!({
        "curseforge_configured": !ext.curseforge_api_key.is_empty(),
        "curseforge_api_key_masked": masked,
        "curseforge_api_key_malformed": malformed,
        "curseforge_api_key_length": key_len,
        "curseforge_api_key_expected_length": CF_API_KEY_LEN,
    })))
}

#[derive(Deserialize)]
struct PutSettingsBody {
    curseforge_api_key: Option<String>,
}

async fn admin_put_settings(
    state: GetState,
    axum::Json(body): axum::Json<PutSettingsBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut settings = state
        .settings
        .get_mut()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    let ext = settings
        .find_mut_extension_settings::<settings::ContentInstallerSettings>()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Settings not found"))?;

    if let Some(key) = body.curseforge_api_key {
        // Trim before storing. A key pasted with a trailing newline is not a legal header
        // value, so reqwest fails at build time and the user gets "CurseForge request
        // failed: builder error" — which says nothing about the real problem (#22).
        // Surrounding spaces are harmless (headers strip OWS) but there is no reason to keep them.
        ext.curseforge_api_key = key.trim().into();
    }

    settings
        .save()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Save failed: {e}")))?;

    Ok(axum::Json(serde_json::json!({ "success": true })))
}

const ALLOWED_DOMAINS: &[&str] = &[
    "https://cdn.modrinth.com/",
    "https://cdn-raw.modrinth.com/",
    "https://edge.forgecdn.net/",
    "https://mediafilez.forgecdn.net/",
    "https://media.forgecdn.net/",
];

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

#[derive(Deserialize)]
struct InstallParams {
    url: String,
    filename: String,
    directory: String,
}

/// POST: Download a plugin/mod file to the server
async fn install_content(
    state: GetState,
    permissions: GetPermissionManager,
    server: GetServer,
    Query(params): Query<InstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions
        .has_server_permission("files.create")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;

    if !ALLOWED_DOMAINS.iter().any(|d| params.url.starts_with(d)) {
        return Err(err(StatusCode::BAD_REQUEST, "URL domain not allowed"));
    }

    let is_datapacks = params.directory.ends_with("/datapacks");
    if params.directory != "plugins" && params.directory != "mods" && !is_datapacks {
        return Err(err(StatusCode::BAD_REQUEST, "Directory must be 'plugins', 'mods', or '<world>/datapacks'"));
    }
    if is_datapacks && params.directory.contains("..") {
        return Err(err(StatusCode::BAD_REQUEST, "Invalid directory path"));
    }

    let filename = params.filename
        .replace('/', "")
        .replace('\\', "")
        .replace("..", "");
    if filename.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Invalid filename"));
    }

    let node = server
        .node
        .fetch_cached(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let wings = node
        .api_client(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let _ = wings
        .post_servers_server_files_create_directory(
            server.uuid,
            &wings_api::servers_server_files_create_directory::post::RequestBody {
                root: "/".into(),
                name: params.directory.clone().into(),
            },
        )
        .await;

    let _ = wings
        .post_servers_server_files_delete(
            server.uuid,
            &wings_api::servers_server_files_delete::post::RequestBody {
                root: format!("/{}", params.directory).into(),
                files: vec![filename.clone().into()],
            },
        )
        .await;

    let pull_result = wings
        .post_servers_server_files_pull(
            server.uuid,
            &wings_api::servers_server_files_pull::post::RequestBody {
                root: format!("/{}", params.directory).into(),
                url: params.url.into(),
                file_name: Some(filename.into()),
                use_header: false,
                foreground: false,
            },
        )
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Wings pull failed: {e:?}")))?;

    let identifier = match pull_result {
        wings_api::servers_server_files_pull::post::Response::Accepted(r) => Some(r.identifier),
        wings_api::servers_server_files_pull::post::Response::Ok(_) => None,
    };

    Ok(axum::Json(serde_json::json!({
        "success": true,
        "identifier": identifier,
    })))
}

/// GET: Check download progress
async fn install_status(
    state: GetState,
    _permissions: GetPermissionManager,
    server: GetServer,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let node = server
        .node
        .fetch_cached(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let wings = node
        .api_client(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let pulls = wings
        .get_servers_server_files_pull(server.uuid)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("{e:?}")))?;

    if let Some(dl) = pulls.downloads.first() {
        Ok(axum::Json(serde_json::json!({
            "state": "downloading",
            "progress": dl.progress,
            "total": dl.total,
        })))
    } else {
        Ok(axum::Json(serde_json::json!({ "state": "done" })))
    }
}

#[derive(Deserialize)]
struct RemoveParams {
    filename: String,
    directory: String,
}

/// POST: Remove a plugin/mod file
async fn remove_content(
    state: GetState,
    permissions: GetPermissionManager,
    server: GetServer,
    Query(params): Query<RemoveParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions
        .has_server_permission("files.delete")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.delete permission"))?;

    let is_datapacks = params.directory.ends_with("/datapacks");
    if params.directory != "plugins" && params.directory != "mods" && !is_datapacks {
        return Err(err(StatusCode::BAD_REQUEST, "Directory must be 'plugins', 'mods', or '<world>/datapacks'"));
    }
    if is_datapacks && params.directory.contains("..") {
        return Err(err(StatusCode::BAD_REQUEST, "Invalid directory path"));
    }

    let filename = params.filename
        .replace('/', "")
        .replace('\\', "")
        .replace("..", "");
    if filename.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Invalid filename"));
    }

    let node = server
        .node
        .fetch_cached(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let wings = node
        .api_client(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    wings
        .post_servers_server_files_delete(
            server.uuid,
            &wings_api::servers_server_files_delete::post::RequestBody {
                root: format!("/{}", params.directory).into(),
                files: vec![filename.into()],
            },
        )
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Wings delete failed: {e:?}")))?;

    Ok(axum::Json(serde_json::json!({ "success": true })))
}

// ─── Modpack Installation ────────────────────────────────────

#[derive(Deserialize)]
struct ModpackInstallParams {
    /// URL to the .mrpack file on cdn.modrinth.com
    mrpack_url: String,
    /// Whether to wipe the server first
    #[serde(default)]
    clean_install: bool,
}

/// POST: Install a Modrinth modpack (.mrpack).
async fn modpack_install(
    state: GetState,
    permissions: GetPermissionManager,
    server: GetServer,
    Query(params): Query<ModpackInstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions
        .has_server_permission("files.create")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;

    // Validate mrpack URL is from Modrinth CDN
    if !params.mrpack_url.starts_with("https://cdn.modrinth.com/") {
        return Err(err(StatusCode::BAD_REQUEST, "mrpack URL must be from cdn.modrinth.com"));
    }

    // Refuse while the server itself is installing/restoring: the panel owns
    // that flag and `Server::install` re-checks it transactionally.
    if server.status.is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "Server is already installing or restoring a backup",
        ));
    }

    server
        .install(
            &state,
            params.clean_install,
            Some(install_script::modrinth_script(&params.mrpack_url)),
        )
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start install: {e}")))?;

    Ok(axum::Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct CfModpackInstallParams {
    /// CurseForge CDN URL to the modpack zip
    zip_url: String,
    #[serde(default)]
    clean_install: bool,
}

/// POST: Install a CurseForge modpack. Same native Wings install flow; the
/// script receives the CurseForge API key through its environment to resolve
/// file download URLs.
async fn cf_modpack_install(
    state: GetState,
    permissions: GetPermissionManager,
    server: GetServer,
    Query(params): Query<CfModpackInstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions
        .has_server_permission("files.create")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;

    if !params.zip_url.starts_with("https://edge.forgecdn.net/")
        && !params.zip_url.starts_with("https://mediafilez.forgecdn.net/")
        && !params.zip_url.starts_with("https://media.forgecdn.net/")
    {
        return Err(err(StatusCode::BAD_REQUEST, "URL must be from CurseForge CDN"));
    }

    if server.status.is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "Server is already installing or restoring a backup",
        ));
    }

    // The install script needs the CurseForge API key to resolve downloads.
    let cf_api_key = {
        let settings_guard = state
            .settings
            .get()
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
        let ext = settings_guard
            .find_extension_settings::<settings::ContentInstallerSettings>()
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Settings not found"))?;
        if ext.curseforge_api_key.is_empty() {
            return Err(err(StatusCode::SERVICE_UNAVAILABLE, "CurseForge API key not configured"));
        }
        ext.curseforge_api_key.to_string()
    };

    server
        .install(
            &state,
            params.clean_install,
            Some(install_script::curseforge_script(&params.zip_url, &cf_api_key)),
        )
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start install: {e}")))?;

    Ok(axum::Json(serde_json::json!({ "success": true })))
}
