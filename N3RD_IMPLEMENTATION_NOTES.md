# N3rdmade Calagopus Content Installer — Implementation Notes

This branch ports the best behavior from `N3rdmade/pelican-modpackmanager` onto the Calagopus-native `content-installer` foundation.

## Completed in phase 1

- Keep the Calagopus-native Rust backend, Wings install flow, Modrinth/CurseForge clients, JAR inspection, update/removal logic, and server detection.
- Replace the single destructive `clean_install` switch with two independent options:
  - **Wipe old server / modpack files**
  - **Delete existing world**
- Detect worlds by `level.dat` instead of assuming `world`, `world_nether`, and `world_the_end`.
- Preserve detected worlds unless world deletion is explicitly selected.
- Preserve operator-owned files during a pack wipe: `server.properties`, EULA, ops, whitelist, bans, and user cache.
- Conservatively preserve unreadable/uncertain directories rather than risking user data.
- Never call Calagopus's blanket clean-install wipe from this extension; cleanup is world-aware before the native install flow starts.
- Log detected worlds and cleanup choices in server activity.
- Require `files.delete` permission when destructive options are selected.
- Existing `clean_install` query clients remain accepted as a backwards-compatible alias for `wipe_files`.

## Next phases

### Phase 2 — runtime / loader / egg planning
- Build a unified install plan containing provider, MC version, loader, loader version, Java version, pack shape, and launcher/runtime strategy.
- Prefer authoritative server-pack launcher/runtime evidence over weak provider tags, especially Forge vs NeoForge on 1.20.1.
- Map Forge / NeoForge / Fabric / Quilt to Calagopus-native eggs and server variables.
- Use Calagopus model/API logic for egg/startup/image/variable changes; do not write directly to Postgres.
- Restore launcher detection for `start.sh`, `run.sh`, `startserver.sh`, `unix_args.txt`, Fabric/Quilt launcher jars, installer jars, and ServerPackCreator-style packs.
- Restore Java image selection by Minecraft/runtime requirements.
- Verify expected runtime files before marking an install usable.

### Phase 3 — provider parity
- Add FTB / modpacks.ch.
- Add ATLauncher.
- Add All Sources merged search with partial-provider failure handling.
- Preserve strict version/loader filtering and required-file failure behavior from the Pelican manager.

### Phase 4 — N3rdmade UI
- Recreate the richer card layout and install flow from the Pelican manager in React.
- Description / Gallery / Open / Install actions.
- Multi-loader and Minecraft-version filters.
- Category pills, active filter chips, sort controls, and Load More.
- Final install confirmation summary showing pack/provider/version/loader plus backup/wipe/world options.

### Phase 5 — backups and polish
- Add create-backup-before-install using Calagopus-native backup APIs.
- Optional delete temporary backup after a successful install.
- Restore selective server.properties preference handling when needed.
- Add server artwork/icon integration if Calagopus exposes a supported native mechanism.
