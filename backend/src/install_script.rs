//! Wings install scripts for modpack installation.

use wings_api::InstallationScript;

/// Non-bash entrypoint: Wings runs `/bin/bash /path/script`.
const CONTAINER_IMAGE: &str = "python:3.12-slim";
const ENTRYPOINT: &str = "/bin/bash";

fn bash_wrapper(python: &str) -> String {
    format!(
        "#!/bin/bash\nset -e\npython3 - <<'CI_INSTALLER_PYTHON'\n{}\nCI_INSTALLER_PYTHON\n",
        python
    )
}

/// Build the install script for a Modrinth `.mrpack` install.
pub fn modrinth_script(mrpack_url: &str) -> InstallationScript {
    let mut environment = indexmap::IndexMap::new();
    environment.insert(
        compact_str::CompactString::from("MRPACK_URL"),
        serde_json::Value::String(mrpack_url.to_string()),
    );

    let script = format!("{}\n{}", PYTHON_COMMON, MODRINTH_PYTHON);

    InstallationScript {
        container_image: compact_str::CompactString::from(CONTAINER_IMAGE),
        entrypoint: compact_str::CompactString::from(ENTRYPOINT),
        script: compact_str::CompactString::from(bash_wrapper(&script)),
        environment,
    }
}

/// Build the install script for a CurseForge modpack install.
pub fn curseforge_script(zip_url: &str, cf_api_key: &str) -> InstallationScript {
    let mut environment = indexmap::IndexMap::new();
    environment.insert(
        compact_str::CompactString::from("CF_ZIP_URL"),
        serde_json::Value::String(zip_url.to_string()),
    );
    environment.insert(
        compact_str::CompactString::from("CF_API_KEY"),
        serde_json::Value::String(cf_api_key.to_string()),
    );

    let script = format!("{}\n{}", PYTHON_COMMON, CURSEFORGE_PYTHON);

    InstallationScript {
        container_image: compact_str::CompactString::from(CONTAINER_IMAGE),
        entrypoint: compact_str::CompactString::from(ENTRYPOINT),
        script: compact_str::CompactString::from(bash_wrapper(&script)),
        environment,
    }
}

const PYTHON_COMMON: &str = r###"import datetime, json, os, re, shutil, sys, time, urllib.request, zipfile
from pathlib import Path

WORKSPACE = Path("/mnt/server")

RETRYABLE = {408, 425, 429, 500, 502, 503, 504}
PROTECTED = (
    "world",
    "world_nether",
    "world_the_end",
    "server.properties",
    "whitelist.json",
    "banned-ips.json",
    "banned-players.json",
    "ops.json",
    "eula.txt",
    ".mcvc-type.json",
)


def log(msg):
    print(f"[content-installer] {msg}", flush=True)


def download(url, dest, headers=None):
    last = None
    for attempt in range(1, 8):
        try:
            req = urllib.request.Request(url, headers=headers or {})
            with urllib.request.urlopen(req, timeout=120) as resp, open(dest, "wb") as out:
                shutil.copyfileobj(resp, out)
            return
        except Exception as e:  # noqa: BLE001
            last = e
            code = getattr(e, "code", None)
            text = str(e).lower()
            retryable = code in RETRYABLE or any(
                k in text
                for k in (
                    "timed out",
                    "timeout",
                    "connection reset",
                    "connection refused",
                    "connection closed",
                    "temporarily unavailable",
                )
            )
            if attempt == 7 or not retryable:
                raise
            delay = 15 if code == 429 else min(2 * (2 ** min(attempt - 1, 3)), 120)
            log(f"download failed ({last}), retrying in {delay}s ({attempt}/7)")
            time.sleep(delay)
    raise last


def get_json(url, headers=None):
    req = urllib.request.Request(url, headers=headers or {})
    with urllib.request.urlopen(req, timeout=120) as resp:
        return json.load(resp)


def is_protected(rel):
    norm = rel.lstrip("/")
    return any(norm == p or norm.startswith(p + "/") for p in PROTECTED)


def is_safe(path):
    if not path or path.startswith(("/", "\\")):
        return False
    if len(path) >= 2 and path[1] == ":":
        return False
    return ".." not in path.replace("\\", "/").split("/")


def extract_safely(z, dest):
    base = os.path.realpath(dest)
    for info in z.infolist():
        name = (info.filename or "").replace("\\", "/")
        if not is_safe(name):
            log(f"skipping unsafe archive path {name}")
            continue
        if is_protected(name):
            log(f"skipping protected archive path {name}")
            continue
        target = os.path.realpath(os.path.join(base, *name.split("/")))
        if target != base and not target.startswith(base + os.sep):
            log(f"skipping archive path escaping workspace {name}")
            continue
        if info.is_dir():
            os.makedirs(target, exist_ok=True)
            continue
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with z.open(info) as src, open(target, "wb") as dst:
            shutil.copyfileobj(src, dst)


def fetch_exclusions():
    fallback = [
        "optifine", "sodium", "iris", "oculus", "rubidium", "embeddium",
        "entityculling", "fpsreducer", "skinlayers3d", "notenoughanimations",
        "ambientsounds", "fancymenu", "drippyloadingscreen", "blur",
        "modmenu", "controlling", "betterf3", "mousetweaks", "freecam",
        "litematica", "minihud", "tweakeroo", "citresewn", "continuity",
        "chatheads", "reauth", "physicsmod", "roughlyenoughitems", "legendarytooltips",
        "betterthirdperson", "dynamiclights", "ryoamiclights", "immediatelyfast", "reforgium",
    ]
    try:
        data = get_json("https://raw.githubusercontent.com/regrave/content-installer/main/client-only-mods.json")
        return [str(x) for x in data.get("excludes", fallback)]
    except Exception as e:  # noqa: BLE001
        log(f"failed to fetch exclusion list ({e}), using fallback")
        return fallback


def known_client_only(filename, exclusions):
    name = filename.rsplit("/", 1)[-1]
    for suffix in (".jar", ".zip"):
        if name.endswith(suffix):
            name = name[: -len(suffix)]
            break
    name = name.lower()
    return any(
        name.startswith(p) or ("-" + p) in name or ("_" + p) in name
        for p in (str(x).lower() for x in exclusions)
    )


def jar_client_only(jar_path):
    try:
        with zipfile.ZipFile(jar_path) as z:
            names = z.namelist()
            if "fabric.mod.json" in names:
                try:
                    data = json.loads(z.read("fabric.mod.json").decode("utf-8", "replace"))
                    if data.get("environment") == "client":
                        return True
                except Exception:  # noqa: BLE001
                    pass
            if "quilt.mod.json" in names:
                try:
                    data = json.loads(z.read("quilt.mod.json").decode("utf-8", "replace"))
                    if (data.get("quilt_loader") or {}).get("environment") == "client":
                        return True
                except Exception:  # noqa: BLE001
                    pass
            for meta in ("META-INF/mods.toml", "META-INF/neoforge.mods.toml"):
                if meta in names:
                    lower = z.read(meta).decode("utf-8", "replace").lower()
                    has_client = 'side="client"' in lower or 'side = "client"' in lower
                    has_both = 'side="both"' in lower or 'side = "both"' in lower
                    has_server = 'side="server"' in lower or 'side = "server"' in lower
                    if has_client and not has_both and not has_server:
                        return True
                    if (
                        'displaytest="ignore_all_version"' in lower
                        or 'displaytest = "ignore_all_version"' in lower
                        or 'displaytest="ignore_server_only"' in lower
                        or 'displaytest = "ignore_server_only"' in lower
                    ):
                        return True
    except Exception:  # noqa: BLE001
        pass
    return False


def mod_id(jar_path):
    try:
        with zipfile.ZipFile(jar_path) as z:
            names = z.namelist()
            if "fabric.mod.json" in names:
                return str(
                    json.loads(z.read("fabric.mod.json").decode("utf-8", "replace")).get("id", "")
                ).lower()
            if "quilt.mod.json" in names:
                return str(
                    json.loads(z.read("quilt.mod.json").decode("utf-8", "replace"))
                    .get("quilt_loader", {})
                    .get("id", "")
                ).lower()
            for meta in ("META-INF/mods.toml", "META-INF/neoforge.mods.toml"):
                if meta in names:
                    text = z.read(meta).decode("utf-8", "replace")
                    for line in text.splitlines():
                        match = re.match(r'\s*modId\s*=\s*"([^"]+)"', line)
                        if match:
                            return match.group(1).lower()
    except Exception:  # noqa: BLE001
        pass
    return ""


def required_dep_ids(mods_dir):
    required = set()
    for jar in mods_dir.glob("*.jar"):
        try:
            with zipfile.ZipFile(jar) as z:
                names = z.namelist()
                if "fabric.mod.json" in names:
                    data = json.loads(z.read("fabric.mod.json").decode("utf-8", "replace"))
                    for dep in (data.get("depends") or {}):
                        required.add(str(dep).lower())
                if "quilt.mod.json" in names:
                    data = json.loads(z.read("quilt.mod.json").decode("utf-8", "replace"))
                    for dep in ((data.get("quilt_loader") or {}).get("depends") or {}):
                        required.add(str(dep).lower())
                for meta in ("META-INF/mods.toml", "META-INF/neoforge.mods.toml"):
                    if meta in names:
                        text = z.read(meta).decode("utf-8", "replace")
                        for dep in re.findall(r'modId\s*=\s*"([^"]+)"', text):
                            required.add(dep.lower())
        except Exception:  # noqa: BLE001
            pass
    return required


def mcjars_zip(kind, mc):
    data = get_json(f"https://versions.mcjars.app/api/v2/builds/{kind}/{mc}")
    builds = data.get("builds") or []
    if not builds:
        raise RuntimeError(f"no {kind} builds available for Minecraft {mc}")
    return builds[0]["zipUrl"]


def apply_overrides(src_dir):
    src = WORKSPACE / src_dir
    if not src.exists():
        return
    for entry in sorted(src.iterdir()):
        name = entry.name
        if is_protected(name):
            log(f"skipping protected override {name}")
            continue
        dst = WORKSPACE / name
        if dst.exists():
            if dst.is_dir():
                shutil.rmtree(dst)
            else:
                dst.unlink()
        shutil.move(str(entry), str(dst))
        log(f"applied override {name}")


def install_loader(url, is_zip, ltype):
    if is_zip:
        download(url, WORKSPACE / "_loader_install.zip")
        with zipfile.ZipFile(WORKSPACE / "_loader_install.zip") as z:
            extract_safely(z, WORKSPACE)
        (WORKSPACE / "_loader_install.zip").unlink(missing_ok=True)
    else:
        download(url, WORKSPACE / "server.jar")
    return ltype


def resolve_loader(deps, mc):
    if "fabric-loader" in deps:
        return (
            f"https://meta.fabricmc.net/v2/versions/loader/{mc}/{deps['fabric-loader']}/1.0.1/server/jar",
            False,
            "FABRIC",
        )
    if "quilt-loader" in deps:
        return (
            f"https://meta.quiltmc.org/v3/versions/loader/{mc}/{deps['quilt-loader']}/0.10.3/server/jar",
            False,
            "QUILT",
        )
    if "neoforge" in deps:
        return (mcjars_zip("NEOFORGE", mc), True, "NEOFORGE")
    if "forge" in deps:
        return (mcjars_zip("FORGE", mc), True, "FORGE")
    return None


def remove_client_only_mods():
    mods_dir = WORKSPACE / "mods"
    if not mods_dir.exists():
        return
    exclusions = fetch_exclusions()
    required = required_dep_ids(mods_dir)
    to_remove = []
    for jar in sorted(mods_dir.glob("*.jar")):
        if mod_id(jar) in required:
            log(f"keeping {jar.name}: required dependency")
            continue
        if jar_client_only(jar):
            to_remove.append((jar, "jar scan"))
        elif known_client_only(jar.name, exclusions):
            to_remove.append((jar, "name list"))
    for jar, reason in to_remove:
        log(f"removing client-only mod {jar.name} (caught by {reason})")
        jar.unlink(missing_ok=True)


def write_marker(loader_type, mc, modpack_name, extra=None):
    marker = {
        "type": loader_type,
        "version": mc,
        "modpack": modpack_name,
        "installedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }
    if extra:
        marker.update(extra)
    (WORKSPACE / "eula.txt").write_text("eula=true\n", encoding="utf-8")
    (WORKSPACE / ".mcvc-type.json").write_text(json.dumps(marker), encoding="utf-8")
"###;

// Modrinth-specific body: env var, host allowlist, and main().
const MODRINTH_PYTHON: &str = r###"from urllib.parse import urlparse

MRPACK_URL = os.environ["MRPACK_URL"]
ALLOWED_HOSTS = (
    "cdn.modrinth.com",
    "cdn-raw.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
    "objects.githubusercontent.com",
)


def allowed_url(url):
    host = (urlparse(url).hostname or "").lower()
    return any(host == d or host.endswith("." + d) for d in ALLOWED_HOSTS)


def main():
    log(f"downloading modpack from {MRPACK_URL}")
    if not allowed_url(MRPACK_URL):
        raise RuntimeError(f"modpack URL host not in allowlist: {MRPACK_URL}")
    download(MRPACK_URL, WORKSPACE / "_mrpack_install.zip")

    log("extracting modpack")
    tmp = WORKSPACE / "_mrpack_temp"
    shutil.rmtree(tmp, ignore_errors=True)
    with zipfile.ZipFile(WORKSPACE / "_mrpack_install.zip") as z:
        extract_safely(z, tmp)

    with open(tmp / "modrinth.index.json", encoding="utf-8") as f:
        index = json.load(f)

    log("applying config overrides")
    apply_overrides("_mrpack_temp/overrides")
    apply_overrides("_mrpack_temp/server-overrides")

    log("checking mod compatibility")
    (WORKSPACE / "mods").mkdir(parents=True, exist_ok=True)

    files = [f for f in index.get("files", []) if (f.get("env") or {}).get("server") != "unsupported"]
    total = len(files)
    skipped = 0
    for i, f in enumerate(files, 1):
        path = f.get("path") or ""
        if not is_safe(path):
            log(f"skipping invalid path {path}")
            skipped += 1
            continue
        if is_protected(path):
            log(f"skipping protected path {path}")
            skipped += 1
            continue
        url = next((u for u in f.get("downloads", []) if allowed_url(u)), None)
        if not url:
            log(f"skipping {path}: no allowed download URL")
            skipped += 1
            continue
        dest = WORKSPACE / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        log(f"downloading {i}/{total}: {path}")
        download(url, dest)

    deps = index.get("dependencies") or {}
    mc = str(deps.get("minecraft", "1.21.1"))
    loader = resolve_loader(deps, mc)

    loader_type = "UNKNOWN"
    if loader:
        log(f"installing {loader[2]} loader")
        loader_type = install_loader(*loader)

    log("scanning for client-only mods")
    remove_client_only_mods()

    write_marker(loader_type, mc, index.get("name", ""))

    log("cleaning up")
    shutil.rmtree(tmp, ignore_errors=True)
    (WORKSPACE / "_mrpack_install.zip").unlink(missing_ok=True)
    log(f"modpack installation complete ({total} files, {skipped} skipped)")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:  # noqa: BLE001
        print(f"[content-installer] install failed: {e}", flush=True)
        sys.exit(1)
"###;

// CurseForge-specific body: env vars, CF API client, and main().
const CURSEFORGE_PYTHON: &str = r###"CF_ZIP_URL = os.environ["CF_ZIP_URL"]
CF_API_KEY = os.environ.get("CF_API_KEY", "")


def post_json(url, body, headers=None):
    data = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        url, data=data, headers={**({"x-api-key": CF_API_KEY, "Accept": "application/json"} if headers is None else headers), "Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        return json.load(resp)


# CurseForge bills per API call, so resolve file metadata in batches instead of
# one GET per manifest entry. POST /v1/mods/files accepts up to 50 fileIds.
def get_cf_files(file_ids):
    if not CF_API_KEY:
        raise RuntimeError("CurseForge API key not configured")
    ids = [fid for fid in file_ids if fid]
    out = {}
    for i in range(0, len(ids), 50):
        chunk = ids[i:i + 50]
        data = post_json(
            "https://api.curseforge.com/v1/mods/files",
            {"fileIds": chunk},
            {"x-api-key": CF_API_KEY, "Accept": "application/json"},
        )
        for entry in data.get("data", []):
            out[entry.get("id")] = entry
    return out


def safe_filename(fn):
    fn = fn.replace("/", "").replace("\\", "").replace("..", "")
    return fn or "unknown.jar"


def main():
    if not CF_API_KEY:
        raise RuntimeError("CurseForge API key not configured")

    log(f"downloading modpack from {CF_ZIP_URL}")
    download(CF_ZIP_URL, WORKSPACE / "_cf_modpack.zip")

    log("extracting modpack")
    tmp = WORKSPACE / "_cf_temp"
    shutil.rmtree(tmp, ignore_errors=True)
    with zipfile.ZipFile(WORKSPACE / "_cf_modpack.zip") as z:
        extract_safely(z, tmp)

    with open(tmp / "manifest.json", encoding="utf-8") as f:
        manifest = json.load(f)

    log("applying config overrides")
    apply_overrides("_cf_temp/" + str(manifest.get("overrides", "overrides")))

    log("checking mod compatibility")
    (WORKSPACE / "mods").mkdir(parents=True, exist_ok=True)

    required = [f for f in manifest.get("files", []) if f.get("required", True)]
    total = len(required)
    downloaded = 0
    skipped = 0
    files_by_id = get_cf_files([cf_file.get("fileID") for cf_file in required])
    for cf_file in required:
        fid = cf_file.get("fileID")
        file_info = files_by_id.get(fid) or {}
        if not file_info:
            log(f"failed to resolve file {fid}, skipping")
            skipped += 1
            continue
        filename = safe_filename(str(file_info.get("fileName") or "unknown.jar"))
        url = file_info.get("downloadUrl")
        if not url:
            log(f"no download URL for {filename}, skipping")
            skipped += 1
            continue
        log(f"downloading ({downloaded + 1}/{total}): {filename}")
        download(url, WORKSPACE / "mods" / filename)
        downloaded += 1

    mc = str(manifest.get("minecraft", {}).get("version", "1.21.1"))
    loaders = manifest.get("minecraft", {}).get("modLoaders") or []
    primary = next((l for l in loaders if l.get("primary")), loaders[0] if loaders else None)
    loader_id = str((primary or {}).get("id", ""))
    deps = {}
    for prefix, key in (
        ("forge-", "forge"),
        ("neoforge-", "neoforge"),
        ("fabric-", "fabric-loader"),
        ("quilt-", "quilt-loader"),
    ):
        if loader_id.startswith(prefix):
            deps[key] = loader_id[len(prefix):]
            break

    loader = resolve_loader(deps, mc)

    loader_type = "UNKNOWN"
    if loader:
        log(f"installing {loader[2]} loader")
        loader_type = install_loader(*loader)

    log("scanning for client-only mods")
    remove_client_only_mods()

    write_marker(loader_type, mc, manifest.get("name", ""), extra={"source": "curseforge"})

    log("cleaning up")
    shutil.rmtree(tmp, ignore_errors=True)
    (WORKSPACE / "_cf_modpack.zip").unlink(missing_ok=True)
    log(f"modpack installation complete ({downloaded} files, {skipped} skipped)")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:  # noqa: BLE001
        print(f"[content-installer] install failed: {e}", flush=True)
        sys.exit(1)
"###;