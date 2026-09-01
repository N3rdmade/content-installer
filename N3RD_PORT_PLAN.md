# N3rdmade Hybrid Content Installer Plan

This branch keeps Calagopus-native Content Installer as the technical base and ports the strongest UX and install-safety features from N3rdmade/pelican-modpackmanager.

## Architectural rule

- Keep Calagopus-native Rust backend, React frontend, Wings install flow, permissions, activity logging, Modrinth/CurseForge proxying, hashing/update management, and server detection where it is stronger.
- Port the Pelican fork's richer browser/install UX and its safer modpack-switching behavior.
- Do not copy Pelican PHP/Laravel code directly when Calagopus already exposes a native mechanism.
- For egg/startup handling, first use Calagopus-native egg/server mutation APIs if available. Only reproduce custom loader/runtime logic when the native API cannot express the behavior.

## Features to keep from current Calagopus Content Installer

- Native .c7s extension architecture.
- Native Wings install state, live console logs, cancellation and refresh-safe progress.
- Modrinth + CurseForge browse/install.
- Modrinth .mrpack handling.
- CurseForge manifest handling.
- Current client-only filtering and JAR metadata inspection.
- Deterministic server loader/version detection.
- Plugins/mods/datapacks support based on detected server type.
- Installed-content detection, update and remove flows.
- Secure CurseForge proxy/settings handling.

## Features to port from N3rdmade Pelican Modpack Manager

### Browser / UX
- All Sources combined search.
- CurseForge, Modrinth, FTB and ATLauncher providers.
- Rich cards with provider badge, loader/version metadata and better visual hierarchy.
- Description, Gallery, Open and Install actions.
- Large in-panel gallery viewer.
- Load-more pagination.
- Minecraft-version filter.
- Multi-loader filters: Forge, NeoForge, Fabric, Quilt.
- Category filters and removable active-filter chips.
- Partial provider failure handling with retry.
- Final install confirmation showing all destructive/preservation options.

### Safe server switching
- Detect existing Minecraft worlds by level.dat rather than hard-coded world names.
- Separate `Wipe existing server files` and `Delete existing world` choices.
- Clean-switch behavior should preserve worlds unless delete-world is explicitly selected.
- Preserve eula, ops, whitelist, bans and user cache.
- Preserve only operational server.properties values; do not carry old worldgen/seed/generator settings into a different pack.
- If keeping an existing world, preserve its level-name.
- Remove stale launchers, manifests, installer jars and loader runtimes before cross-pack installs.
- Do not silently skip required server files.
- Repair mode-000 extracted files/directories.

### Providers
- FTB/modpacks.ch support.
- ATLauncher support.
- Prefer official CurseForge server packs when available.
- Build server packs from client manifests when needed.
- Strict provider/loader/version filtering.

### Loader / runtime / startup
- Correct Forge, NeoForge, Fabric and Quilt selection.
- Exact loader-version handling.
- Correct Minecraft and Java version selection.
- Detect self-contained server packs and use their launchers when appropriate.
- Recognize start.sh, run.sh, startserver.sh, unix_args.txt, Fabric/Quilt launcher jars and installer metadata.
- Avoid stale launcher metadata from a previous pack.
- Reset startup/image/variables when changing runtime type.
- Verify expected runtime files before declaring install complete.
- Handle unlimited-memory startup without -Xmx0M.

## New Calagopus-specific work

1. Audit Calagopus server/egg APIs and determine the native equivalent of Pelican EggChangerService.
2. Add a backend install-plan structure that carries provider, pack/version, Minecraft version, loader, loader version, Java version, server-pack mode, wipe-files and delete-world choices.
3. Add native server settings/egg/startup mutation before/after the Wings install script as appropriate.
4. Move world/file preservation into our install script rather than relying blindly on Calagopus `clean_install=true`, because the old plugin intentionally separates world deletion from file cleanup.
5. Add a small install-state marker for same-pack update detection so destructive defaults can differ between a new pack and an update/reinstall.

## Initial implementation order

1. Safety parity: wipe-files vs delete-world, world detection, server.properties selective preservation, stale launcher/runtime cleanup.
2. Loader/version/startup/egg handling using Calagopus-native APIs.
3. Rich modpack browser UI based on the Pelican fork's visual behavior.
4. Provider expansion: FTB + ATLauncher + All Sources.
5. Mods/plugins browser UX parity and category filters.
6. Artwork/icon integration if Calagopus exposes a server-image field or extension hook.

## Important behavior difference discovered

Current Content Installer passes `clean_install` directly into Calagopus `server.install(...)`. The Pelican fork had more nuanced behavior: wiping pack files and deleting worlds were independent choices, and clean pack switching preserved detected worlds by default. The hybrid should not keep using a single destructive boolean as the final design.
