import { useEffect } from 'react';
import { uploadFiles } from '@/lib/files/uploadManager.ts';

const ARTWORK_ATTR = 'data-n3rdmade-server-artwork';
const PENDING_ARTWORK_PREFIX = 'n3rdmade:server-artwork:';
const objectUrls = new Map<string, string>();
const syncing = new Set<string>();

function serverIdFromLink(link: HTMLAnchorElement): string | null {
  const href = link.getAttribute('href') ?? '';
  const match = href.match(/^\/server\/([^/?#]+)/);
  return match?.[1] ?? null;
}

function currentServerId(): string | null {
  const match = window.location.pathname.match(/^\/server\/([^/?#]+)/);
  return match?.[1] ?? null;
}

function pendingKey(serverId: string): string {
  return `${PENDING_ARTWORK_PREFIX}${serverId}`;
}

function getPendingArtwork(serverId: string): string | null {
  try {
    return localStorage.getItem(pendingKey(serverId));
  } catch {
    return null;
  }
}

function setPendingArtwork(serverId: string, url: string) {
  try {
    localStorage.setItem(pendingKey(serverId), url);
  } catch {
    // Browser storage is best-effort; the install should never fail because of artwork.
  }
}

function clearPendingArtwork(serverId: string) {
  try {
    localStorage.removeItem(pendingKey(serverId));
  } catch {
    // Ignore storage failures.
  }
}

async function loadServerIcon(serverId: string): Promise<string | null> {
  const cached = objectUrls.get(serverId);
  if (cached) return cached;

  const params = new URLSearchParams({ file: '/server-icon.png' });
  const response = await fetch(`/api/client/servers/${serverId}/files/contents?${params}`);
  if (!response.ok) return null;

  const blob = await response.blob();
  if (blob.size === 0) return null;

  const url = URL.createObjectURL(blob);
  objectUrls.set(serverId, url);
  return url;
}

async function makeMinecraftIcon(url: string): Promise<File> {
  const response = await fetch(url, { credentials: 'omit' });
  if (!response.ok) throw new Error(`Artwork download failed (${response.status})`);

  const blob = await response.blob();
  const bitmap = await createImageBitmap(blob);
  try {
    const canvas = document.createElement('canvas');
    canvas.width = 64;
    canvas.height = 64;
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Canvas unavailable');

    const scale = Math.max(64 / bitmap.width, 64 / bitmap.height);
    const width = bitmap.width * scale;
    const height = bitmap.height * scale;
    const x = (64 - width) / 2;
    const y = (64 - height) / 2;
    ctx.drawImage(bitmap, x, y, width, height);

    const png = await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob((value) => value ? resolve(value) : reject(new Error('PNG conversion failed')), 'image/png');
    });
    return new File([png], 'server-icon.png', { type: 'image/png' });
  } finally {
    bitmap.close();
  }
}

async function syncPendingArtwork(serverId: string, serverName: string, url: string) {
  if (syncing.has(serverId)) return;
  syncing.add(serverId);
  try {
    const file = await makeMinecraftIcon(url);
    await uploadFiles({
      type: 'server',
      serverUuid: serverId,
      serverName,
      routeId: serverId,
      directory: '/',
    }, [file]);

    const old = objectUrls.get(serverId);
    if (old) URL.revokeObjectURL(old);
    objectUrls.delete(serverId);
    clearPendingArtwork(serverId);
  } catch (error) {
    // Keep the pending URL so a later dashboard visit can retry automatically.
    console.warn('[N3rdmade Content Manager] Could not sync server artwork:', error);
  } finally {
    syncing.delete(serverId);
  }
}

function insertArtwork(title: HTMLElement, src: string) {
  if (!title.parentElement || title.parentElement.querySelector(`img[${ARTWORK_ATTR}]`)) return;

  const image = document.createElement('img');
  image.setAttribute(ARTWORK_ATTR, 'true');
  image.src = src;
  image.alt = '';
  image.loading = 'lazy';
  image.style.width = '42px';
  image.style.height = '42px';
  image.style.objectFit = 'cover';
  image.style.borderRadius = '9px';
  image.style.flexShrink = '0';
  image.style.border = '1px solid var(--mantine-color-default-border)';
  image.style.background = 'var(--mantine-color-default-hover)';

  title.parentElement.insertBefore(image, title);
}

async function enhanceLink(link: HTMLAnchorElement) {
  if (link.dataset.n3rdmadeArtworkChecked === 'true') return;
  link.dataset.n3rdmadeArtworkChecked = 'true';

  const serverId = serverIdFromLink(link);
  if (!serverId) return;

  const title = link.querySelector('span.text-xl');
  if (!(title instanceof HTMLElement)) return;

  const pending = getPendingArtwork(serverId);
  if (pending) {
    insertArtwork(title, pending);
    void syncPendingArtwork(serverId, title.textContent?.trim() || 'Server', pending);
    return;
  }

  try {
    const src = await loadServerIcon(serverId);
    if (src) insertArtwork(title, src);
  } catch {
    // Missing or unreadable server-icon.png should simply keep the stock card.
  }
}

function captureSelectedModpackArtwork(event: MouseEvent) {
  const target = event.target;
  if (!(target instanceof Element)) return;
  const button = target.closest('button');
  if (!button || button.disabled || !button.textContent?.includes('Install Modpack')) return;

  const serverId = currentServerId();
  if (!serverId) return;

  const modal = button.closest('[role="dialog"]') ?? document;
  const icon = modal.querySelector<HTMLImageElement>('img.ci-detail-icon');
  const src = icon?.currentSrc || icon?.src;
  if (!src || !/^https?:\/\//i.test(src)) return;

  // The install may wipe the current root files. Remember the chosen pack art now;
  // the dashboard writes a fresh Minecraft-compatible server-icon.png afterward.
  setPendingArtwork(serverId, src);
}

function scan() {
  document
    .querySelectorAll<HTMLAnchorElement>('a.block.min-w-0[href^="/server/"]')
    .forEach((link) => void enhanceLink(link));
}

export default function ServerArtworkEnhancer() {
  useEffect(() => {
    scan();

    const observer = new MutationObserver(() => scan());
    observer.observe(document.body, { childList: true, subtree: true });
    document.addEventListener('click', captureSelectedModpackArtwork, true);

    return () => {
      observer.disconnect();
      document.removeEventListener('click', captureSelectedModpackArtwork, true);
    };
  }, []);

  return null;
}
