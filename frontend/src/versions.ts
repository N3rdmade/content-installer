// Minecraft game-version formatting shared by the Browse and Modpacks tabs.

/**
 * A shipped release: purely numeric, dot separated. Deliberately excludes snapshots
 * (`26w03a`), pre-releases (`1.21.2-pre1`) and release candidates, all of which show up
 * in Modrinth's `game_versions` and CurseForge's `gameVersions`. CurseForge additionally
 * mixes loader and runtime labels ("Forge", "Java 17") into the same array.
 */
const RELEASE_VERSION = /^\d+(?:\.\d+)*$/;

/**
 * Order two Minecraft versions.
 *
 * Compares component-wise as numbers rather than as text, which matters twice over:
 * `1.9` must sort below `1.10`, and since Mojang moved to year-based numbering in 2026
 * the line runs 1.21.11 -> 26.1 -> 26.2, so `26.2` has to outrank every `1.x`.
 * Missing components count as zero, so `26.1` sits below `26.1.2`.
 */
function compareVersions(a: string, b: string): number {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const diff = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/**
 * Collapse a version list into `lowest - highest`, or a single version when a file
 * targets exactly one. Returns '' when nothing in the list is a release, which callers
 * use to drop the parenthetical entirely.
 *
 * Note this describes the span of what the file declares support for, not a guarantee
 * that every intermediate version is covered.
 */
export function formatVersionRange(versions: string[]): string {
  const releases = versions.filter((v) => RELEASE_VERSION.test(v)).sort(compareVersions);
  if (releases.length === 0) return '';

  const lowest = releases[0];
  const highest = releases[releases.length - 1];
  return lowest === highest ? lowest : `${lowest} - ${highest}`;
}

/** `2.6.0 (1.20.5 - 26.2)`, or just `2.6.0` when no release versions are declared. */
export function versionLabel(name: string, versions: string[]): string {
  const range = formatVersionRange(versions);
  return range ? `${name} (${range})` : name;
}
