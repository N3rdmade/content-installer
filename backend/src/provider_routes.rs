use axum::{extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use shared::{
    GetState,
    models::{
        server::{GetServer, GetServerActivityLogger},
        user::GetPermissionManager,
    },
};
use std::collections::HashSet;

use crate::{atlauncher, ftb, provider_install, runtime, settings::ContentInstallerSettings};

const PRESERVED_SERVER_FILES: &[&str] = &[
    "server.properties", "eula.txt", "ops.json", "whitelist.json",
    "banned-ips.json", "banned-players.json", "usercache.json",
];

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

async fn list_dir(
    wings: &wings_api::client::WingsClient,
    uuid: uuid::Uuid,
    path: &str,
) -> Result<Vec<wings_api::DirectoryEntry>, (StatusCode, String)> {
    let response = wings.get_servers_server_files_list(
        uuid,
        &wings_api::servers_server_files_list::get::Query {
            directory: Some(path.into()),
            per_page: Some(1000),
            page: Some(1),
            ..Default::default()
        },
    ).await.map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Wings file listing failed: {e:?}")))?;
    Ok(response.entries)
}

async fn safe_cleanup(
    wings: &wings_api::client::WingsClient,
    uuid: uuid::Uuid,
    wipe_files: bool,
    delete_world: bool,
) -> Result<Vec<String>, (StatusCode, String)> {
    if !wipe_files && !delete_world { return Ok(Vec::new()); }
    let root = list_dir(wings, uuid, "/").await?;
    let mut worlds = HashSet::new();
    let mut uncertain = HashSet::new();
    for entry in root.iter().filter(|entry| entry.directory) {
        match list_dir(wings, uuid, &format!("/{}", entry.name)).await {
            Ok(children) if children.iter().any(|child| child.file && child.name.as_str() == "level.dat") => {
                worlds.insert(entry.name.to_string());
            }
            Err(_) => { uncertain.insert(entry.name.to_string()); }
            _ => {}
        }
    }

    let mut delete = Vec::new();
    for entry in &root {
        let name = entry.name.as_str();
        if worlds.contains(name) {
            if delete_world { delete.push(entry.name.clone()); }
        } else if wipe_files
            && !PRESERVED_SERVER_FILES.contains(&name)
            && !(entry.directory && uncertain.contains(name))
        {
            delete.push(entry.name.clone());
        }
    }
    if !delete.is_empty() {
        wings.post_servers_server_files_delete(
            uuid,
            &wings_api::servers_server_files_delete::post::RequestBody { root: "/".into(), files: delete },
        ).await.map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Wings cleanup failed: {e:?}")))?;
    }
    let mut worlds = worlds.into_iter().collect::<Vec<_>>();
    worlds.sort();
    Ok(worlds)
}

async fn cf_key(state: &GetState) -> Option<String> {
    let guard = state.settings.get().await.ok()?;
    let settings = guard.find_extension_settings::<ContentInstallerSettings>().ok()?;
    (!settings.curseforge_api_key.is_empty()).then(|| settings.curseforge_api_key.to_string())
}

#[derive(Deserialize)]
pub struct FtbInstallParams {
    pack_id: u64,
    version_id: u64,
    #[serde(default)]
    wipe_files: bool,
    #[serde(default)]
    delete_world: bool,
    #[serde(default)]
    modpack_name: Option<String>,
    #[serde(default)]
    version_name: Option<String>,
}

pub async fn ftb_install(
    state: GetState,
    permissions: GetPermissionManager,
    mut server: GetServer,
    activity: GetServerActivityLogger,
    Query(params): Query<FtbInstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions.has_server_permission("files.create").map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;
    permissions.has_server_permission("settings.install").map_err(|_| err(StatusCode::FORBIDDEN, "Missing settings.install permission"))?;
    if params.wipe_files || params.delete_world {
        permissions.has_server_permission("files.delete").map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.delete permission"))?;
    }
    if server.status.is_some() { return Err(err(StatusCode::CONFLICT, "Server is already installing or restoring")); }

    let manifest = ftb::install_manifest(params.pack_id, params.version_id).await?;
    let node = server.node.fetch_cached(&state.database).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let wings = node.api_client(&state.database).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let worlds = safe_cleanup(&wings, server.uuid, params.wipe_files, params.delete_world).await?;

    let runtime = if let Some(loader) = manifest.loader.as_deref() {
        Some(runtime::apply(
            &state,
            &mut server.0,
            loader,
            manifest.minecraft.as_deref(),
            manifest.loader_version.as_deref(),
            manifest.java,
        ).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Runtime switch failed: {e}")))?)
    } else { None };

    let files = manifest.files.into_iter().map(|file| provider_install::ProviderFile {
        name: file.name,
        dir: file.dir,
        url: file.url,
        cf_project: file.cf_project,
        cf_file: file.cf_file,
    }).collect();
    let pack_name = params.modpack_name.unwrap_or_else(|| format!("FTB {}", params.pack_id));
    let version_name = params.version_name.unwrap_or_else(|| params.version_id.to_string());
    let script = provider_install::script(provider_install::ProviderInstallPlan {
        provider: "ftb".into(),
        pack_name: pack_name.clone(),
        version_name: version_name.clone(),
        files,
        configs_url: None,
        loader: manifest.loader.clone(),
        minecraft: manifest.minecraft.clone(),
        loader_version: manifest.loader_version.clone(),
        curseforge_api_key: cf_key(&state).await,
    }).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Install plan failed: {e}")))?;

    server.install(&state, false, Some(script)).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start install: {e}")))?;
    activity.log("server:content-installer.modpack.install", serde_json::json!({
        "source": "ftb", "modpack": pack_name, "version": version_name,
        "runtime": runtime, "wipe_files": params.wipe_files, "delete_world": params.delete_world,
        "detected_worlds": worlds,
    })).await;
    Ok(axum::Json(serde_json::json!({"success": true})))
}

#[derive(Deserialize)]
pub struct AtInstallParams {
    safe_name: String,
    version: String,
    #[serde(default)]
    wipe_files: bool,
    #[serde(default)]
    delete_world: bool,
    #[serde(default)]
    modpack_name: Option<String>,
}

pub async fn atlauncher_install(
    state: GetState,
    permissions: GetPermissionManager,
    mut server: GetServer,
    activity: GetServerActivityLogger,
    Query(params): Query<AtInstallParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    permissions.has_server_permission("files.create").map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.create permission"))?;
    permissions.has_server_permission("settings.install").map_err(|_| err(StatusCode::FORBIDDEN, "Missing settings.install permission"))?;
    if params.wipe_files || params.delete_world {
        permissions.has_server_permission("files.delete").map_err(|_| err(StatusCode::FORBIDDEN, "Missing files.delete permission"))?;
    }
    if server.status.is_some() { return Err(err(StatusCode::CONFLICT, "Server is already installing or restoring")); }

    let manifest = atlauncher::install_manifest(&params.safe_name, &params.version).await?;
    let node = server.node.fetch_cached(&state.database).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let wings = node.api_client(&state.database).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let worlds = safe_cleanup(&wings, server.uuid, params.wipe_files, params.delete_world).await?;

    let runtime = if let Some(loader) = manifest.loader.as_deref() {
        Some(runtime::apply(
            &state,
            &mut server.0,
            loader,
            manifest.minecraft.as_deref(),
            manifest.loader_version.as_deref(),
            manifest.java,
        ).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Runtime switch failed: {e}")))?)
    } else { None };

    let files = manifest.files.into_iter().map(|file| provider_install::ProviderFile {
        name: file.name, dir: file.dir, url: Some(file.url), cf_project: None, cf_file: None,
    }).collect();
    let pack_name = params.modpack_name.unwrap_or_else(|| params.safe_name.clone());
    let script = provider_install::script(provider_install::ProviderInstallPlan {
        provider: "atlauncher".into(),
        pack_name: pack_name.clone(),
        version_name: params.version.clone(),
        files,
        configs_url: manifest.configs_url.clone(),
        loader: manifest.loader.clone(),
        minecraft: manifest.minecraft.clone(),
        loader_version: manifest.loader_version.clone(),
        curseforge_api_key: None,
    }).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Install plan failed: {e}")))?;

    server.install(&state, false, Some(script)).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start install: {e}")))?;
    activity.log("server:content-installer.modpack.install", serde_json::json!({
        "source": "atlauncher", "modpack": pack_name, "version": params.version,
        "runtime": runtime, "wipe_files": params.wipe_files, "delete_world": params.delete_world,
        "detected_worlds": worlds,
    })).await;
    Ok(axum::Json(serde_json::json!({"success": true})))
}
