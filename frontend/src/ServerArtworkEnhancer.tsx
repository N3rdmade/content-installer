import { useEffect } from 'react';

const ARTWORK_ATTR = 'data-n3rdmade-server-artwork';
const PENDING_ARTWORK_PREFIX = 'n3rdmade:server-artwork:';
const objectUrls = new Map<string, string>();
const syncing = new Set<string>();

interface PendingArtwork {
  url: string;
  serverUuid: string;
  routeId: string;
  serverName: string;
}

function serverIdFromLink(link: HTMLAnchorElement): string | null {
  const href = link.getAttribute('href') ?? '';
  const match = href.match(/^\/server\/([^/?#]+)/);
  return match?.[1] ?? null;
}

function currentServerId(): string | null {
  const match = window.location.pathname.match(/^\/server\/([^/?#]+)/);
  return match?.[1] ?? null;
}

function pendingKey(routeId: string): string {
  return `${PENDING_ARTWORK_PREFIX}${routeId}`;
}

function getPendingArtwork(routeId: string): PendingArtwork | null {
  try {
    const raw = localStorage.getItem(pendingKey(routeId));
    if (!raw) return null;

    if (/^https?:\/\//i.test(raw)) {
      return { url: raw, serverUuid: routeId, routeId, serverName: 'Server' };
    }

    const value = JSON.parse(raw) as Partial<PendingArtwork>;
    if (!value.url) return null;
    return {
      url: value.url,
      serverUuid: value.serverUuid || routeId,
      routeId: value.routeId || routeId,
      serverName: value.serverName || 'Server',
    };
  } catch {
    return null;
  }
}

export function queueServerArtwork(
  serverUuid: string,
  routeId: string,
  serverName: string,
  url: string,
) {
  if (!/^https?:\/\//i.test(url)) return;
  try {
    localStorage.setItem(
      pendingKey(routeId),
      JSON.stringify({ url, serverUuid, routeId, serverName } satisfies PendingArtwork),
    );
  } catch {
    // Browser storage is best-effort; the install should never fail because of artwork.
  }
}

function clearPendingArtwork(routeId: string) {
  try {
    localStorage.removeItem(pendingKey(routeId));
  } catch {
    // Ignore storage failures.
  }
}

async function resolveServerIdentity(pending: PendingArtwork): Promise<PendingArtwork> {
  try {
    const response = await fetch(`/api/client/servers/${pending.routeId}`);
    if (!response.ok) return pending;
    const data = await response.json() as Record<string, unknown>;
    return {
      ...pending,
      serverUuid: typeof data.uuid === 'string' ? data.uuid : pending.serverUuid,
      serverName: typeof data.name === 'string' ? data.name : pending.serverName,
    };
  } catch {
    return pending;
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

async function uploadServerIcon(serverUuid: string, file: File): Promise<void> {
  const ticketResponse = await fetch(`/api/client/servers/${serverUuid}/files/upload`);
  if (!ticketResponse.ok) {
    throw new Error(`Could not create upload ticket (${ticketResponse.status})`);
  }

  const ticket = await ticketResponse.json() as { url?: string };
  if (!ticket.url) throw new Error('Upload ticket did not include a URL');

  const separator = ticket.url.includes('?') ? '&' : '?';
  const uploadUrl = `${ticket.url}${separator}directory=${encodeURIComponent('/')}`;
  const form = new FormData();
  form.append('files', file, 'server-icon.png');

  const uploadResponse = await fetch(uploadUrl, {
    method: 'POST',
    body: form,
  });
  if (!uploadResponse.ok) {
    throw new Error(`server-icon.png upload failed (${uploadResponse.status})`);
  }
}

async function syncPendingArtwork(rawPending: PendingArtwork) {
  if (syncing.has(rawPending.routeId)) return;
  syncing.add(rawPending.routeId);
  try {
    const pending = await resolveServerIdentity(rawPending);
    const file = await makeMinecraftIcon(pending.url);
    await uploadServerIcon(pending.serverUuid, file);

    const old = objectUrls.get(pending.routeId);
    if (old) URL.revokeObjectURL(old);
    objectUrls.delete(pending.routeId);
    clearPendingArtwork(pending.routeId);
  } catch (error) {
    // Keep the pending data so a later dashboard visit can retry automatically.
    console.warn('[N3rdmade Content Manager] Could not sync server artwork:', error);
  } finally {
    syncing.delete(rawPending.routeId);
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

  const routeId = serverIdFromLink(link);
  if (!routeId) return;

  const title = link.querySelector('span.text-xl');
  if (!(title instanceof HTMLElement)) return;

  const pending = getPendingArtwork(routeId);
  if (pending) {
    insertArtwork(title, pending.url);
    void syncPendingArtwork(pending);
    return;
  }

  try {
    const src = await loadServerIcon(routeId);
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

  const routeId = currentServerId();
  if (!routeId) return;

  const modal = button.closest('[role="dialog"]') ?? document;
  const icon = modal.querySelector<HTMLImageElement>('img.ci-detail-icon');
  const src = icon?.currentSrc || icon?.src;
  if (!src || !/^https?:\/\//i.test(src)) return;

  queueServerArtwork(routeId, routeId, 'Server', src);
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
