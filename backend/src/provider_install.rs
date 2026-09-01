use wings_api::InstallationScript;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderFile {
    pub name: String,
    pub dir: String,
    pub url: Option<String>,
    pub cf_project: Option<u64>,
    pub cf_file: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProviderInstallPlan {
    pub provider: String,
    pub pack_name: String,
    pub version_name: String,
    pub files: Vec<ProviderFile>,
    pub configs_url: Option<String>,
    pub loader: Option<String>,
    pub minecraft: Option<String>,
    pub loader_version: Option<String>,
    pub curseforge_api_key: Option<String>,
}

const CONTAINER_IMAGE: &str = "python:3.12-slim";
const ENTRYPOINT: &str = "/bin/bash";

pub fn script(plan: ProviderInstallPlan) -> Result<InstallationScript, serde_json::Error> {
    let files = serde_json::to_string(&plan.files)?;
    let meta = serde_json::to_string(&serde_json::json!({
        "provider": plan.provider,
        "pack_name": plan.pack_name,
        "version_name": plan.version_name,
        "configs_url": plan.configs_url,
        "loader": plan.loader,
        "minecraft": plan.minecraft,
        "loader_version": plan.loader_version,
    }))?;

    let mut environment = indexmap::IndexMap::new();
    environment.insert(
        "CF_API_KEY".into(),
        serde_json::Value::String(plan.curseforge_api_key.unwrap_or_default()),
    );

    let python = format!(
        r###"import datetime, hashlib, json, os, pathlib, re, shutil, sys, time, urllib.request, urllib.parse, zipfile

WORKSPACE = pathlib.Path('/mnt/server')
FILES = json.loads({files:?})
META = json.loads({meta:?})
CF_API_KEY = os.environ.get('CF_API_KEY', '')
RETRYABLE = {{408, 425, 429, 500, 502, 503, 504}}


def log(msg):
    print(f"[n3rdmade-installer] {{msg}}", flush=True)


def safe_rel(value):
    value = str(value or '').replace('\\', '/').strip('/')
    if not value or value.startswith('/') or '..' in value.split('/'):
        return None
    if len(value) >= 2 and value[1] == ':':
        return None
    return value


def download(url, destination, headers=None):
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != 'https' or not parsed.hostname:
        raise RuntimeError(f'unsafe download URL: {{url}}')
    destination.parent.mkdir(parents=True, exist_ok=True)
    last = None
    for attempt in range(1, 8):
        try:
            req = urllib.request.Request(url, headers=headers or {{}})
            with urllib.request.urlopen(req, timeout=120) as response, open(destination, 'wb') as out:
                shutil.copyfileobj(response, out)
            return
        except Exception as exc:
            last = exc
            code = getattr(exc, 'code', None)
            retryable = code in RETRYABLE or any(word in str(exc).lower() for word in (
                'timed out', 'timeout', 'connection reset', 'connection refused', 'temporarily unavailable'))
            if attempt == 7 or not retryable:
                raise
            delay = 15 if code == 429 else min(2 ** attempt, 60)
            log(f'download failed ({{exc}}), retrying in {{delay}}s')
            time.sleep(delay)
    raise last


def post_json(url, body, headers=None):
    data = json.dumps(body).encode('utf-8')
    req = urllib.request.Request(url, data=data, headers={{**(headers or {{}}), 'Content-Type': 'application/json'}})
    with urllib.request.urlopen(req, timeout=120) as response:
        return json.load(response)


def get_json(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers={{'User-Agent': 'N3rdmade/content-installer'}}), timeout=120) as response:
        return json.load(response)


def resolve_curseforge_urls(files):
    unresolved = [f for f in files if not f.get('url') and f.get('cf_file')]
    if not unresolved:
        return
    if not CF_API_KEY:
        raise RuntimeError('This FTB pack contains CurseForge-only files but no CurseForge API key is configured.')
    ids = [int(f['cf_file']) for f in unresolved]
    metadata = {{}}
    for offset in range(0, len(ids), 50):
        result = post_json(
            'https://api.curseforge.com/v1/mods/files',
            {{'fileIds': ids[offset:offset + 50]}},
            {{'x-api-key': CF_API_KEY, 'Accept': 'application/json'}},
        )
        for item in result.get('data', []):
            metadata[int(item.get('id', 0))] = item
    for file in unresolved:
        item = metadata.get(int(file['cf_file'])) or {{}}
        url = item.get('downloadUrl')
        if not url:
            raise RuntimeError(f"CurseForge does not expose a server-download URL for {{file.get('name')}}")
        file['url'] = url


def extract_zip(url, name):
    archive = WORKSPACE / name
    download(url, archive)
    base = WORKSPACE.resolve()
    with zipfile.ZipFile(archive) as z:
        for info in z.infolist():
            rel = safe_rel(info.filename)
            if not rel:
                continue
            target = (WORKSPACE / rel).resolve()
            if target != base and base not in target.parents:
                continue
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with z.open(info) as src, open(target, 'wb') as dst:
                    shutil.copyfileobj(src, dst)
    archive.unlink(missing_ok=True)


def mcjars_zip(kind, mc, requested):
    data = get_json(f'https://versions.mcjars.app/api/v2/builds/{{kind}}/{{mc}}')
    builds = data.get('builds') or []
    if not builds:
        raise RuntimeError(f'No {{kind}} build exists for Minecraft {{mc}}')
    exact = next((b for b in builds if str(b.get('projectVersionId', '')) == str(requested or '') or str(b.get('name', '')) == str(requested or '')), None)
    chosen = exact or builds[0]
    return chosen.get('zipUrl')


def install_loader():
    loader = str(META.get('loader') or '').lower()
    mc = str(META.get('minecraft') or '')
    version = str(META.get('loader_version') or '')
    if not loader or not mc:
        log('No loader metadata supplied; pack files were installed but runtime loader was not changed.')
        return
    log(f'Installing runtime: {{loader}} {{version or "latest"}} / Minecraft {{mc}}')
    if loader == 'fabric':
        if not version:
            data = get_json('https://meta.fabricmc.net/v2/versions/loader')
            version = str(data[0]['version'])
        url = f'https://meta.fabricmc.net/v2/versions/loader/{{mc}}/{{version}}/1.0.1/server/jar'
        download(url, WORKSPACE / 'server.jar')
    elif loader == 'quilt':
        if not version:
            data = get_json('https://meta.quiltmc.org/v3/versions/loader')
            version = str(data[0]['version'])
        url = f'https://meta.quiltmc.org/v3/versions/loader/{{mc}}/{{version}}/0.10.3/server/jar'
        download(url, WORKSPACE / 'server.jar')
    elif loader in ('forge', 'neoforge'):
        url = mcjars_zip(loader.upper(), mc, version)
        if not url:
            raise RuntimeError(f'No downloadable {{loader}} runtime found')
        extract_zip(url, '_runtime.zip')
        (WORKSPACE / 'user_jvm_args.txt').write_text('-Xms128M\n-XX:MaxRAMPercentage=92.5\n', encoding='utf-8')
    else:
        log(f'Loader {{loader}} is not handled by the modpack runtime installer.')


def write_marker():
    marker = {{
        'type': str(META.get('loader') or 'UNKNOWN').upper(),
        'version': META.get('minecraft'),
        'loaderVersion': META.get('loader_version'),
        'provider': META.get('provider'),
        'modpack': META.get('pack_name'),
        'modpackVersion': META.get('version_name'),
        'installedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }}
    (WORKSPACE / '.mcvc-type.json').write_text(json.dumps(marker), encoding='utf-8')
    (WORKSPACE / '.n3rdmade-modpack.json').write_text(json.dumps(marker, indent=2), encoding='utf-8')
    (WORKSPACE / 'eula.txt').write_text('eula=true\n', encoding='utf-8')


def main():
    log(f"Installing {{META.get('pack_name')}} ({{META.get('version_name')}}) from {{META.get('provider')}}")
    resolve_curseforge_urls(FILES)
    total = len(FILES)
    for index, file in enumerate(FILES, 1):
        name = safe_rel(file.get('name'))
        directory = safe_rel(file.get('dir')) if file.get('dir') else ''
        url = file.get('url')
        if not name or not url:
            raise RuntimeError(f'Invalid required file entry: {{file}}')
        rel = f'{{directory}}/{{name}}' if directory else name
        target = WORKSPACE / rel
        log(f'Downloading {{index}}/{{total}}: {{rel}}')
        download(url, target)

    configs = META.get('configs_url')
    if configs:
        log('Applying ATLauncher configuration bundle')
        extract_zip(configs, '_configs.zip')

    install_loader()
    write_marker()
    log(f'Modpack installation complete ({{total}} files).')


if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        print(f'[n3rdmade-installer] install failed: {{exc}}', flush=True)
        sys.exit(1)
"###
    );

    let script = format!(
        "#!/bin/bash\nset -e\npython3 - <<'N3RD_INSTALLER_PYTHON'\n{}\nN3RD_INSTALLER_PYTHON\n",
        python
    );

    Ok(InstallationScript {
        container_image: CONTAINER_IMAGE.into(),
        entrypoint: ENTRYPOINT.into(),
        script: script.into(),
        environment,
    })
}
