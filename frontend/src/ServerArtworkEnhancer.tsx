import { useEffect } from 'react';

const ARTWORK_ATTR = 'data-n3rdmade-server-artwork';
const objectUrls = new Map<string, string>();

function serverIdFromLink(link: HTMLAnchorElement): string | null {
  const href = link.getAttribute('href') ?? '';
  const match = href.match(/^\/server\/([^/?#]+)/);
  return match?.[1] ?? null;
}

async function loadServerIcon(serverId: string): Promise<string | null> {
  const cached = objectUrls.get(serverId);
  if (cached) return cached;

  const params = new URLSearchParams({ file: '/server-icon.png' });
  const response = await fetch(`/api/client/servers/${serverId}/files/contents?${params}`);
  if (!response.ok) return null;

  const blob = await response.blob();
  if (!blob.type.startsWith('image/') && blob.size === 0) return null;

  const url = URL.createObjectURL(blob);
  objectUrls.set(serverId, url);
  return url;
}

async function enhanceLink(link: HTMLAnchorElement) {
  if (link.dataset.n3rdmadeArtworkChecked === 'true') return;
  link.dataset.n3rdmadeArtworkChecked = 'true';

  const serverId = serverIdFromLink(link);
  if (!serverId) return;

  const title = link.querySelector('span.text-xl');
  if (!(title instanceof HTMLElement)) return;
  if (title.parentElement?.querySelector(`img[${ARTWORK_ATTR}]`)) return;

  try {
    const src = await loadServerIcon(serverId);
    if (!src || !title.parentElement) return;

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
  } catch {
    // Missing or unreadable server-icon.png should simply keep the stock card.
  }
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

    return () => observer.disconnect();
  }, []);

  return null;
}
