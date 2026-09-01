mod atlauncher;
mod curseforge;
mod ftb;
mod install_script;
mod provider_install;
mod provider_routes;
mod runtime;
mod settings;

use axum::{extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use shared::{
    GetState,
    extensions::{Extension, ExtensionRouteBuilder},
    models::{
        server::{GetServer, GetServerActivityLogger},
        user::GetPermissionManager,
    },
    State,
};
use std::{collections::HashSet, sync::Arc};

const CF_API_KEY_LEN: usize = 60;

const PRESERVED_SERVER_FILES: &[&str] = &[
    "server.properties",
    "eula.txt",
    "ops.json",
    "whitelist.json",
    "banned-ips.json",
    "banned-players.json",
    "usercache.json",
];

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
                        "/content-installer/ftb/search",
                        axum::routing::get(ftb::search),
                    )
                    .route(
                        "/content-installer/ftb/versions",
                        axum::routing::get(ftb::versions),
                    )
                    .route(
                        "/content-installer/atlauncher/search",
                        axum::routing::get(atlauncher::search),
                    )
                    .route(
                        "/content-installer/atlauncher/versions",
                        axum::routing::get(atlauncher::versions),
                    )
                    .route(
                        "/content-installer/runtime/prepare",
                        axum::routing::post(provider_routes::prepare_runtime),
                    )
                    .route(
                        "/content-installer/modpack/install",
                        axum::routing::post(modpack_install),
                    )
                    .route(
                        "/content-installer/modpack/cf-install",
                        axum::routing::post(cf_modpack_install),
                    )
                    .route(
                        "/content-installer/modpack/ftb-install",
                        axum::routing::post(provider_routes::ftb_install),
                    )
                    .route(
                        "/content-installer/modpack/atlauncher-install",
                        axum::routing::post(provider_routes::atlauncher_install),
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
        ext.curseforge_api_key = key.trim().into();
    }

    settings
        .save()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Save failed: {e}")))?;

    Ok(axum::Json(serde_json::json!({ "success": true })))
}

const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "cdn-raw.modrinth.com",
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
    "media.forgecdn.net",
];

const MODRINTH_PACK_HOSTS: &[&str] = &["cdn.modrinth.com"];
const CURSEFORGE_PACK_HOSTS: &[&str] = &[
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
    "media.forgecdn.net",
];

fn is_https_url_from(value: &str, allowed_hosts: &[&str]) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| allowed_hosts.contains(&host))
    })
}

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

async fn list_server_directory(
    wings: &wings_api::client::WingsClient,
    server_uuid: uuid::Uuid,
    directory: &str,
) -> Result<Vec<wings_api::DirectoryEntry>, (StatusCode, String)> {
    let result = wings
        .get_servers_server_files_list(
            server_uuid,
            &wings_api::servers_server_files_list::get::Query {
                directory: Some(directory.into()),
                per_page: Some(1000),
                page: Some(1),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Wings file listing failed: {e:?}")))?;

    Ok(result.entries)
}

async fn detect_worlds_and_uncertain_dirs(
    wings: &wings_api::client::WingsClient,
    server_uuid: uuid::Uuid,
    root_entries: &[wings_api::DirectoryEntry],
) -> (HashSet<String>, HashSet<String>) {
    let mut worlds = HashSet::new();
    let mut uncertain = HashSet::new();

    for entry in root_entries.iter().filter(|entry| entry.directory) {
        let name = entry.name.to_string();
        let path = format!("/{name}");
        match list_server_directory(wings, server_uuid, &path).await {
            Ok(entries) => {
                if entries.iter().any(|child| child.file && child.name.as_str() == "level.dat") {
                    worlds.insert(name);
                }
            }
            Err(_) => {
                uncertain.insert(name);
            }
        }
    }

    (worlds, uncertain)
}

async fn prepare_server_for_modpack(
    wings: &wings_api::client::WingsClient,
    server_uuid: uuid::Uuid,
    wipe_files: bool,
    delete_world: bool,
) -> Result<Vec<String>, (StatusCode, String)> {
    if !wipe_files && !delete_world {
        return Ok(Vec::new());
    }

    let root_entries = list_server_directory(wings, server_uuid, "/").await?;
    let (worlds, uncertain_dirs) =
        detect_worlds_and_uncertain_dirs(wings, server_uuid, &root_entries).await;

    let mut delete_names: Vec<compact_str::CompactString> = Vec::new();

    for entry in &root_entries {
        let name = entry.name.as_str();
        let is_world = worlds.contains(name);

        if is_world {
            if delete_world {
                delete_names.push(entry.name.clone());
            }
            continue;
        }

        if wipe_files {
            if PRESERVED_SERVER_FILES.contains(&name) {
                continue;
            }
            if entry.directory && uncertain_dirs.contains(name) {
                continue;
            }
            delete_names.push(entry.name.clone());
        }
    }

    if !delete_names.is_empty() {
        wings
            .post_servers_server_files_delete(
                server_uuid,
                &wings_api::servers_server_files_delete::post::RequestBody {
                    root: "/".into(),
                    files: delete_names,
                },
            )
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Wings cleanup failed: {e:?}")))?;
    }

    let mut detected_worlds: Vec<String> = worlds.into_iter().collect();
    detected_worlds.sort();
    Ok(detected_worlds)
}

#[derive(Deserialize)]
struct InstallParams {
    url: String,
    filename: String,
    directory: String,
}

async fn install_content(
    state: GetState,
    permissions: GetPermissionManager,
    server: GetServer,
    Query(params): Query<InstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions
        .has_server_permission("files.create")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;

    if !is_https_url_from(&params.url, ALLOWED_DOWNLOAD_HOSTS) {
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

#[derive(Deserialize)]
struct RuntimeHints {
    #[serde(default)]
    loader: Option<String>,
    #[serde(default)]
    minecraft: Option<String>,
    #[serde(default)]
    loader_version: Option<String>,
    #[serde(default)]
    java: Option<u8>,
}

async fn apply_runtime_hints(
    state: &GetState,
    permissions: &GetPermissionManager,
    server: &mut GetServer,
    hints: &RuntimeHints,
) -> Result<Option<runtime::AppliedRuntime>, (StatusCode, String)> {
    let Some(loader) = hints.loader.as_deref().filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    permissions
        .has_server_permission("startup.command")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing startup.command permission for runtime switch"))?;

    runtime::apply(
        state,
        &mut server.0,
        loader,
        hints.minecraft.as_deref(),
        hints.loader_version.as_deref(),
        hints.java,
    )
    .await
    .map(Some)
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Runtime switch failed: {e}")))
}

// ─── Modpack Installation ────────────────────────────────────

#[derive(Deserialize)]
struct ModpackInstallParams {
    mrpack_url: String,
    #[serde(default, alias = "clean_install")]
    wipe_files: bool,
    #[serde(default)]
    delete_world: bool,
    #[serde(default)]
    modpack_name: Option<String>,
    #[serde(default)]
    version_name: Option<String>,
    #[serde(flatten)]
    runtime: RuntimeHints,
}

fn install_label(value: Option<&str>, fallback: &str) -> String {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);

    value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

async fn modpack_install(
    state: GetState,
    permissions: GetPermissionManager,
    mut server: GetServer,
    activity_logger: GetServerActivityLogger,
    Query(params): Query<ModpackInstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions
        .has_server_permission("files.create")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;
    permissions
        .has_server_permission("settings.install")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing settings.install permission"))?;
    if params.wipe_files || params.delete_world {
        permissions
            .has_server_permission("files.delete")
            .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.delete permission"))?;
    }

    if !is_https_url_from(&params.mrpack_url, MODRINTH_PACK_HOSTS) {
        return Err(err(StatusCode::BAD_REQUEST, "mrpack URL must be from cdn.modrinth.com"));
    }

    if server.status.is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "Server is already installing or restoring a backup",
        ));
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
    let detected_worlds = prepare_server_for_modpack(
        &wings,
        server.uuid,
        params.wipe_files,
        params.delete_world,
    )
    .await?;

    let applied_runtime = apply_runtime_hints(&state, &permissions, &mut server, &params.runtime).await?;

    let modpack_name = install_label(params.modpack_name.as_deref(), "Modrinth modpack");
    let version_name = install_label(params.version_name.as_deref(), "Selected version");

    server
        .install(
            &state,
            false,
            Some(install_script::modrinth_script(
                &params.mrpack_url,
                &modpack_name,
                &version_name,
            )),
        )
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to start install: {e}"),
            )
        })?;

    activity_logger
        .log(
            "server:content-installer.modpack.install",
            serde_json::json!({
                "source": "modrinth",
                "modpack": modpack_name,
                "version": version_name,
                "runtime": applied_runtime,
                "wipe_files": params.wipe_files,
                "delete_world": params.delete_world,
                "detected_worlds": detected_worlds,
            }),
        )
        .await;

    Ok(axum::Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct CfModpackInstallParams {
    zip_url: String,
    #[serde(default, alias = "clean_install")]
    wipe_files: bool,
    #[serde(default)]
    delete_world: bool,
    #[serde(default)]
    modpack_name: Option<String>,
    #[serde(default)]
    version_name: Option<String>,
    #[serde(flatten)]
    runtime: RuntimeHints,
}

async fn cf_modpack_install(
    state: GetState,
    permissions: GetPermissionManager,
    mut server: GetServer,
    activity_logger: GetServerActivityLogger,
    Query(params): Query<CfModpackInstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions
        .has_server_permission("files.create")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;
    permissions
        .has_server_permission("settings.install")
        .map_err(|_| err(StatusCode::FORBIDDEN, "Missing settings.install permission"))?;
    if params.wipe_files || params.delete_world {
        permissions
            .has_server_permission("files.delete")
            .map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.delete permission"))?;
    }

    if !is_https_url_from(&params.zip_url, CURSEFORGE_PACK_HOSTS) {
        return Err(err(StatusCode::BAD_REQUEST, "URL must be from CurseForge CDN"));
    }

    if server.status.is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "Server is already installing or restoring a backup",
        ));
    }

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

    let node = server
        .node
        .fetch_cached(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    let wings = node
        .api_client(&state.database)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    let detected_worlds = prepare_server_for_modpack(
        &wings,
        server.uuid,
        params.wipe_files,
        params.delete_world,
    )
    .await?;

    let applied_runtime = apply_runtime_hints(&state, &permissions, &mut server, &params.runtime).await?;

    let modpack_name = install_label(params.modpack_name.as_deref(), "CurseForge modpack");
    let version_name = install_label(params.version_name.as_deref(), "Selected version");

    server
        .install(
            &state,
            false,
            Some(install_script::curseforge_script(
                &params.zip_url,
                &cf_api_key,
                &modpack_name,
                &version_name,
            )),
        )
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to start install: {e}"),
            )
        })?;

    activity_logger
        .log(
            "server:content-installer.modpack.install",
            serde_json::json!({
                "source": "curseforge",
                "modpack": modpack_name,
                "version": version_name,
                "runtime": applied_runtime,
                "wipe_files": params.wipe_files,
                "delete_world": params.delete_world,
                "detected_worlds": detected_worlds,
            }),
        )
        .await;

    Ok(axum::Json(serde_json::json!({ "success": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_urls_require_exact_https_hosts() {
        assert!(is_https_url_from(
            "https://cdn.modrinth.com/data/example/versions/1/pack.mrpack",
            MODRINTH_PACK_HOSTS,
        ));
        assert!(!is_https_url_from(
            "https://cdn.modrinth.com.evil.example/pack.mrpack",
            MODRINTH_PACK_HOSTS,
        ));
        assert!(!is_https_url_from(
            "http://cdn.modrinth.com/pack.mrpack",
            MODRINTH_PACK_HOSTS,
        ));
        assert!(is_https_url_from(
            "https://edge.forgecdn.net/files/1234/pack.zip",
            CURSEFORGE_PACK_HOSTS,
        ));
        assert!(!is_https_url_from(
            "https://edge.forgecdn.net.evil.example/pack.zip",
            CURSEFORGE_PACK_HOSTS,
        ));
    }

    #[test]
    fn install_labels_are_bounded_and_console_safe() {
        let label = install_label(Some(" Pack\nName\u{0000} "), "fallback");
        assert_eq!(label, "PackName");
        assert_eq!(install_label(Some("  "), "fallback"), "fallback");
        assert_eq!(install_label(Some(&"x".repeat(200)), "fallback").len(), 160);
    }
}
