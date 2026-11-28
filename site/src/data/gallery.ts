import { KISEKI_BUNDLE } from './bundle';
import { THEME_ORDER, WORLD_META } from './worlds';
import type {
  GalleryIndexPayload,
  GallerySortKey,
  GalleryThemeEntry,
  GalleryUpdatesPayload,
} from '../types/theme';

export const GALLERY_REGISTRY_REPO = 'https://github.com/nerv-base/kiseki-themes';

export const GALLERY_INDEX_URLS: string[] = [
  'https://dl.kisekiterm.dev/gallery/index.json',
  'https://kiseki-dl.nerv-base.com/gallery/index.json',
  'https://nerv-base.github.io/kiseki-themes/index.json',
];

/** Official 4 built-in themes adapted into Gallery cards so the storefront is always rich. */
export const OFFICIAL_GALLERY_THEMES: GalleryThemeEntry[] = THEME_ORDER.map((id, idx) => {
  const pack = KISEKI_BUNDLE.themes[id];
  const meta = WORLD_META[id];
  const p = pack.tokens.palette;
  const s = pack.tokens.syntax;
  return {
    id,
    name: pack.manifest.name,
    subtitle: pack.manifest.subtitle,
    version: pack.manifest.version,
    appearance: 'dark',
    tags: ['official', 'built-in', 'live-shader', meta.shortEn.toLowerCase()],
    authors: [{ name: 'Kiseki Core', url: 'https://github.com/nerv-base/kiseki-terminal' }],
    license: 'MIT',
    repo: 'nerv-base/kiseki-terminal',
    subpath: `themes/${id}`,
    createdAt: '2026-11-26T00:00:00Z',
    updatedAt: '2026-11-28T00:00:00Z',
    builtin: true,
    capabilities: {
      hasWallpaper: Boolean(pack.wallpaperUrl),
      hasShader: Boolean(pack.shaderGlsl),
      shaderProfiles: ['eco', 'balanced', 'ultra'],
      wcagGrade: 'AA',
    },
    metrics: {
      stars: 0,
      stars7d: 0,
      downloads: 0,
      downloads30d: 0,
      trendingScore: 0.01 * (THEME_ORDER.length - idx),
    },
    preview: {
      thumbUrl: pack.thumbUrl || pack.wallpaperUrl,
      palette: {
        base: p.surfaces.base,
        surface0: p.surfaces.surface0,
        textPrimary: p.text.primary,
        accentPrimary: p.accents.primary,
        accentSecondary: p.accents.secondary,
        keyword: s.keyword || p.accents.secondary,
        string: s.string || p.ansi.green,
        function: s.function || p.accents.primary,
      },
    },
  };
});

export interface GalleryFetchResult {
  communityThemes: GalleryThemeEntry[];
  generatedAt: string | null;
  sourceUrl: string | null;
  offline: boolean;
}

async function fetchJsonWithTimeout<T>(url: string, timeoutMs = 8000): Promise<T> {
  const ctrl = new AbortController();
  const timer = window.setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const res = await fetch(url, { signal: ctrl.signal, headers: { Accept: 'application/json' } });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as T;
  } finally {
    window.clearTimeout(timer);
  }
}

export async function fetchGalleryIndex(): Promise<GalleryFetchResult> {
  for (const url of GALLERY_INDEX_URLS) {
    try {
      const payload = await fetchJsonWithTimeout<GalleryIndexPayload>(url);
      if (payload && Array.isArray(payload.themes)) {
        return {
          communityThemes: payload.themes.map((t) => ({ ...t, builtin: false })),
          generatedAt: payload.generatedAt || null,
          sourceUrl: url,
          offline: false,
        };
      }
    } catch {
      // Try next mirror silently.
    }
  }
  return {
    communityThemes: [],
    generatedAt: null,
    sourceUrl: null,
    offline: true,
  };
}

export async function fetchGalleryUpdates(sourceUrl: string | null): Promise<GalleryUpdatesPayload | null> {
  if (!sourceUrl) return null;
  const idx = sourceUrl.lastIndexOf('/');
  const updatesUrl = idx >= 0 ? `${sourceUrl.slice(0, idx)}/updates.json` : 'updates.json';
  try {
    return await fetchJsonWithTimeout<GalleryUpdatesPayload>(updatesUrl, 6000);
  } catch {
    return null;
  }
}

export interface GalleryQueryOptions {
  scope: 'all' | 'community' | 'official';
  query: string;
  appearance: 'all' | 'dark' | 'light';
  onlyShader: boolean;
  onlyWallpaper: boolean;
  sort: GallerySortKey;
}

export function filterAndSortGallery(
  entries: GalleryThemeEntry[],
  opts: GalleryQueryOptions,
): GalleryThemeEntry[] {
  const q = opts.query.trim().toLowerCase();
  const filtered = entries.filter((item) => {
    if (opts.scope === 'community' && item.builtin) return false;
    if (opts.scope === 'official' && !item.builtin) return false;
    if (opts.appearance !== 'all' && item.appearance !== opts.appearance) return false;
    if (opts.onlyShader && !item.capabilities.hasShader) return false;
    if (opts.onlyWallpaper && !item.capabilities.hasWallpaper) return false;
    if (q) {
      const hay = [
        item.id,
        item.name,
        item.subtitle,
        item.repo,
        ...item.tags,
        ...item.authors.map((a) => a.name),
      ]
        .join(' ')
        .toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });

  return [...filtered].sort((a, b) => {
    // Keep community vs built-in fair while breaking ties deterministically.
    let diff = 0;
    switch (opts.sort) {
      case 'trending':
        diff = (b.metrics.trendingScore || 0) - (a.metrics.trendingScore || 0);
        if (diff === 0) diff = (b.metrics.downloads30d || 0) - (a.metrics.downloads30d || 0);
        if (diff === 0) diff = (b.metrics.stars || 0) - (a.metrics.stars || 0);
        break;
      case 'downloads':
        diff = (b.metrics.downloads || 0) - (a.metrics.downloads || 0);
        if (diff === 0) diff = (b.metrics.downloads30d || 0) - (a.metrics.downloads30d || 0);
        break;
      case 'stars':
        diff = (b.metrics.stars || 0) - (a.metrics.stars || 0);
        if (diff === 0) diff = (b.metrics.stars7d || 0) - (a.metrics.stars7d || 0);
        break;
      case 'updated':
        diff = (b.updatedAt || '').localeCompare(a.updatedAt || '');
        break;
      case 'newest':
        diff = (b.createdAt || '').localeCompare(a.createdAt || '');
        break;
    }
    if (diff !== 0) return diff;
    return a.id.localeCompare(b.id);
  });
}

export function formatCompactCount(n: number): string {
  if (n >= 1000000) return `${(n / 1000000).toFixed(1)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}
