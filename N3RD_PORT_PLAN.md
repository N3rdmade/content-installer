# N3rdmade Hybrid Content Installer Plan

This branch keeps Calagopus-native Content Installer as the technical base and ports the strongest UX and install-safety features from N3rdmade/pelican-modpackmanager.

## Current implementation status

- [x] Separate wipe-files and delete-world controls.
- [x] level.dat based world discovery and conservative preservation.
- [x] Calagopus-native runtime planner for egg/startup/image selection.
- [x] Minecraft-to-Java fallback selection (8/17/21) with provider override.
- [x] Runtime hints wired into Modrinth and CurseForge installs.
- [x] FTB/modpacks.ch browse, versions and install-manifest backend.
- [x] ATLauncher browse, versions and Configs.json install-manifest backend.
- [x] FTB + ATLauncher native Wings installation scripts.
- [x] All Sources browser with partial-provider failure handling.
- [x] Rich provider cards, descriptions, artwork/gallery strip, Open action and install plan.
- [x] Minecraft-version filter and multi-loader Forge/NeoForge/Fabric/Quilt filters.
- [x] Active filter indicators, clear filters and Load More.
- [x] Native Calagopus safety-backup-before-install UI with completion wait.
- [x] Separate destructive acknowledgement for server-file replacement/world deletion.
- [x] Hybrid now has its own package ID: `gg.n3rdmade.contentmanager`.
- [x] Hybrid frontend has its own `/content-manager` server page.
- [x] Compile tested on Calagopus Heavy 1.1.5 before the package-ID split.
- [ ] Namespace the hybrid backend API routes so original Content Installer and hybrid can be active in the same binary without overlapping-route panic.
- [ ] Selectively preserve operational server.properties values instead of preserving the whole file.
- [ ] Add same-pack install marker so updates default to no wipe while cross-pack installs default to wipe.
- [ ] Refresh/remap egg variables after changing loader/egg, matching the useful behavior of Pelican EggChangerService.
- [ ] Detect self-contained/server-pack launchers (`start.sh`, `run.sh`, `startserver.sh`, `unix_args.txt`, Fabric/Quilt launch jars) and select startup accordingly.
- [ ] Remove stale launcher/runtime artifacts when switching loader families.
- [ ] Verify expected runtime/launcher files before considering an install usable.
- [ ] Repair extracted mode-000 files/directories when required.
- [ ] Optional cleanup of a temporary pre-install backup after successful install.
- [ ] Live destructive/non-destructive install matrix on Calagopus Heavy before merging.

## Architectural rule

- Keep Calagopus-native Rust backend, React frontend, Wings install flow, permissions, activity logging, Modrinth/CurseForge proxying, hashing/update management, and server detection where it is stronger.
- Port the Pelican fork's richer browser/install UX and its safer modpack-switching behavior.
- Do not copy Pelican PHP/Laravel code directly when Calagopus already exposes a native mechanism.
- For egg/startup handling, first use Calagopus-native egg/server mutation APIs if available. Only reproduce custom loader/runtime logic when the native API cannot express the behavior.

## Features retained from Calagopus Content Installer

- Native `.c7s` extension architecture.
- Native Wings install state and server install workflow.
- Modrinth + CurseForge browse/install.
- Modrinth `.mrpack` handling.
- CurseForge manifest handling.
- Client-only filtering and JAR metadata inspection.
- Deterministic server loader/version detection.
- Plugins/mods/datapacks support based on detected server type.
- Installed-content detection, update and remove flows.
- Secure CurseForge proxy/settings handling.

## Features ported from N3rdmade Pelican Modpack Manager

### Browser / UX
- All Sources combined search.
- CurseForge, Modrinth, FTB and ATLauncher providers.
- Rich cards with provider badge and visual hierarchy.
- Description, gallery/artwork, Open and Install actions.
- Load-more pagination.
- Minecraft-version filter.
- Multi-loader filters: Forge, NeoForge, Fabric, Quilt.
- Active filter indicators and clear-filter action.
- Partial provider failure handling.
- Final install confirmation showing runtime and destructive/preservation options.

### Safe server switching
- Detect existing Minecraft worlds by `level.dat` rather than hard-coded world names.
- Separate `Wipe old server / modpack files` and `Delete existing world` choices.
- Preserve detected worlds unless delete-world is explicitly selected.
- Preserve eula, ops, whitelist, bans and user cache.
- Require explicit acknowledgement before destructive work.
- Optional native Calagopus safety backup before any install changes are started.

### Providers / runtime
- FTB/modpacks.ch support.
- ATLauncher support.
- Forge / NeoForge / Fabric / Quilt runtime planning.
- Correct Minecraft-to-Java fallback selection.
- Matching installed Calagopus eggs when available.
- Runtime startup/image update through Calagopus model APIs.
- Unlimited-memory startup uses `MaxRAMPercentage` rather than `-Xmx0M`.

## Remaining parity work before merge

1. **Selective `server.properties` preservation** — retain operational/admin settings but do not carry seed/world-generation/generator settings into an unrelated pack; retain `level-name` when preserving a world.
2. **Same-pack detection** — persist provider + pack id + selected version/loader marker. Updating/reinstalling the same pack should default wipe off; changing packs should default wipe on.
3. **Egg variable parity** — after an egg switch, reconcile the server variable set with the new egg and populate Minecraft/loader-version variables where the egg exposes them.
4. **Launcher/runtime finalization** — inspect installed files after assembly, detect self-contained launchers and exact Forge/NeoForge/Fabric/Quilt runtime shape, remove stale launchers from previous loader families, and choose the startup command from actual installed evidence.
5. **Runtime verification** — fail clearly if required launcher/runtime files were not produced instead of leaving a server that cannot boot.
6. **Permission repair** — repair extracted mode-000 files/directories without broadly chmodding valid permissions.
7. **Backend API namespace** — move hybrid API/admin routes away from `/content-installer/...` so the stock extension and hybrid can coexist for A/B testing. Until this is complete, test one extension archive at a time.
8. **Live test matrix** — Modrinth, CurseForge, FTB and ATLauncher; Forge, NeoForge, Fabric and Quilt; preserve-world and delete-world; safety backup; same-pack update; cross-pack switch.

## Important behavior difference discovered

Stock Content Installer passes one clean-install switch into the native server install. The Pelican manager was more nuanced: replacing pack files, deleting worlds, preserving player/admin state, changing the loader runtime, and determining same-pack vs cross-pack behavior were separate decisions. The hybrid keeps those concerns separate instead of collapsing them into one destructive boolean.
