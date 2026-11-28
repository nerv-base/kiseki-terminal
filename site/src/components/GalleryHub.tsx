import { useEffect, useMemo, useState, type FC } from 'react';
import {
  GALLERY_REGISTRY_REPO,
  OFFICIAL_GALLERY_THEMES,
  fetchGalleryIndex,
  fetchGalleryUpdates,
  filterAndSortGallery,
  formatCompactCount,
} from '../data/gallery';
import type {
  DocChapterId,
  GallerySortKey,
  GalleryThemeEntry,
  GalleryUpdatesPayload,
  Lang,
  ThemeId,
} from '../types/theme';

interface GalleryHubProps {
  lang: Lang;
  onTryOfficialTheme: (id: ThemeId) => void;
  onOpenDocs: (chapter: DocChapterId) => void;
  onNavigate: (target: 'home' | 'download') => void;
}

const LivePaletteTerminalCard: FC<{ theme: GalleryThemeEntry; tall?: boolean }> = ({
  theme,
  tall = false,
}) => {
  const pal = theme.preview.palette;
  const bg = pal.base || '#131722';
  const surface = pal.surface0 || '#1D2333';
  const fg = pal.textPrimary || '#E9EEF8';
  const accent = pal.accentPrimary || '#74C7EC';
  const accent2 = pal.accentSecondary || '#F5A9B8';
  const kw = pal.keyword || '#CBA6F7';
  const str = pal.string || '#A6E3A1';
  const fn = pal.function || '#89B4FA';

  return (
    <div
      className={`w-full h-full flex flex-col justify-between font-k-mono select-none overflow-hidden ${
        tall ? 'p-5 text-xs' : 'p-3.5 text-[0.6875rem]'
      }`}
      style={{ background: bg, color: fg }}
    >
      <div className="flex items-center justify-between pb-2 border-b" style={{ borderColor: surface }}>
        <div className="flex items-center gap-1.5">
          <span className="w-2 h-2 rounded-full inline-block" style={{ background: accent2 }} />
          <span className="w-2 h-2 rounded-full inline-block" style={{ background: accent }} />
          <span className="w-2 h-2 rounded-full inline-block" style={{ background: str }} />
        </div>
        <span className="opacity-70 tracking-wider text-[0.625rem]">{theme.id}</span>
      </div>

      <div className="my-auto py-2 space-y-1 leading-relaxed">
        <div>
          <span style={{ color: accent }}>❯</span>{' '}
          <span style={{ color: fn }}>kiseki</span>{' '}
          <span style={{ color: fg }}>use {theme.id}</span>
        </div>
        <div>
          <span style={{ color: kw }}>const</span>{' '}
          <span style={{ color: fg }}>world</span>{' '}
          <span style={{ color: accent }}>=</span>{' '}
          <span style={{ color: fn }}>compile</span>(
          <span style={{ color: str }}>&quot;{theme.name}&quot;</span>);
        </div>
        <div>
          <span style={{ color: str }}>✓</span>{' '}
          <span className="opacity-80">
            WCAG {theme.capabilities.wcagGrade || 'AA'} · {theme.appearance.toUpperCase()}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-1 pt-2 border-t" style={{ borderColor: surface }}>
        {[bg, surface, accent, accent2, kw, fn, str, fg].map((hex, i) => (
          <span
            key={`${hex}-${i}`}
            className="h-2 flex-1"
            style={{ background: hex }}
            title={hex}
          />
        ))}
      </div>
    </div>
  );
};

const CardPreview: FC<{ theme: GalleryThemeEntry; tall?: boolean }> = ({ theme, tall = false }) => {
  const [imgFailed, setImgFailed] = useState(false);
  const thumb = theme.preview.thumbUrl;

  useEffect(() => {
    setImgFailed(false);
  }, [thumb]);

  if (thumb && !imgFailed) {
    return (
      <div className="relative w-full h-full overflow-hidden" style={{ background: theme.preview.palette.base || '#0B0E18' }}>
        <img
          src={thumb}
          alt={`${theme.name} terminal screenshot`}
          loading="lazy"
          onError={() => setImgFailed(true)}
          className="w-full h-full object-cover transition-transform duration-500 group-hover:scale-[1.03]"
        />
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            background:
              'linear-gradient(180deg, transparent 45%, color-mix(in srgb, #000 72%, transparent) 100%)',
          }}
        />
      </div>
    );
  }
  return <LivePaletteTerminalCard theme={theme} tall={tall} />;
};

export const GalleryHub: FC<GalleryHubProps> = ({
  lang,
  onTryOfficialTheme,
  onOpenDocs,
  onNavigate,
}) => {
  const isZh = lang === 'zh';

  const [communityThemes, setCommunityThemes] = useState<GalleryThemeEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [sourceUrl, setSourceUrl] = useState<string | null>(null);
  const [offline, setOffline] = useState(false);
  const [updatesMap, setUpdatesMap] = useState<GalleryUpdatesPayload | null>(null);

  const [scope, setScope] = useState<'all' | 'community' | 'official'>('all');
  const [query, setQuery] = useState('');
  const [appearance, setAppearance] = useState<'all' | 'dark' | 'light'>('all');
  const [onlyShader, setOnlyShader] = useState(false);
  const [onlyWallpaper, setOnlyWallpaper] = useState(false);
  const [sort, setSort] = useState<GallerySortKey>('trending');

  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [selected, setSelected] = useState<GalleryThemeEntry | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    fetchGalleryIndex().then((res) => {
      if (!active) return;
      setCommunityThemes(res.communityThemes);
      setSourceUrl(res.sourceUrl);
      setOffline(res.offline);
      setLoading(false);
    });
    return () => {
      active = false;
    };
  }, []);

  // Lazy-load updates.json only when the user opens a community theme detail modal.
  useEffect(() => {
    if (!selected || selected.builtin || updatesMap || !sourceUrl) return;
    let active = true;
    fetchGalleryUpdates(sourceUrl).then((payload) => {
      if (active && payload) setUpdatesMap(payload);
    });
    return () => {
      active = false;
    };
  }, [selected, updatesMap, sourceUrl]);

  useEffect(() => {
    if (!selected) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSelected(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selected]);

  const allEntries = useMemo(
    () => [...communityThemes, ...OFFICIAL_GALLERY_THEMES],
    [communityThemes],
  );

  const visible = useMemo(
    () =>
      filterAndSortGallery(allEntries, {
        scope,
        query,
        appearance,
        onlyShader,
        onlyWallpaper,
        sort,
      }),
    [allEntries, scope, query, appearance, onlyShader, onlyWallpaper, sort],
  );

  const copyCommand = (id: string, cmd: string) => {
    const done = () => {
      setCopiedId(id);
      window.setTimeout(() => setCopiedId((cur) => (cur === id ? null : cur)), 1400);
    };
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(cmd).then(done).catch(done);
    } else {
      done();
    }
  };

  const installCommandFor = (item: GalleryThemeEntry) =>
    item.builtin ? `kiseki use ${item.id}` : `kiseki theme install ${item.id} && kiseki use ${item.id}`;

  const sortOptions: Array<{ key: GallerySortKey; zh: string; en: string }> = [
    { key: 'trending', zh: '综合热门', en: 'Trending' },
    { key: 'downloads', zh: '最多下载', en: 'Downloads' },
    { key: 'stars', zh: '最多 Star', en: 'Stars' },
    { key: 'updated', zh: '最近更新', en: 'Updated' },
    { key: 'newest', zh: '最新上架', en: 'Newest' },
  ];

  return (
    <section
      id="k-view-gallery"
      className="relative z-10 min-h-screen pt-24 pb-24 px-6 sm:px-12 lg:px-14"
      style={{ background: 'var(--k-crust)' }}
    >
      <div className="max-w-[92rem] mx-auto">
        {/* Top bar */}
        <div
          className="flex flex-wrap items-end justify-between gap-6 pb-8 border-b"
          style={{ borderColor: 'color-mix(in srgb, var(--k-border) 45%, transparent)' }}
        >
          <div className="space-y-2.5 max-w-2xl">
            <div className="flex items-center gap-3 font-k-mono text-xs tracking-[0.22em] uppercase" style={{ color: 'var(--k-accent-primary)' }}>
              <span>KISEKI GALLERY</span>
              <span className="opacity-40">//</span>
              <span style={{ color: 'var(--k-text-muted)' }}>
                {isZh
                  ? `${communityThemes.length} 社区 · ${OFFICIAL_GALLERY_THEMES.length} 官方内置`
                  : `${communityThemes.length} community · ${OFFICIAL_GALLERY_THEMES.length} built-in`}
              </span>
            </div>
            <h1 className="font-k-display text-3xl sm:text-4xl tracking-wider" style={{ color: 'var(--k-text-primary)' }}>
              {isZh ? '奇蹟端末 主题市场' : 'Kiseki Theme Gallery'}
            </h1>
            <p className="text-sm leading-relaxed" style={{ color: 'var(--k-text-secondary)' }}>
              {isZh
                ? '社区主题全部经过白名单沙箱、naga Shader 与 WCAG 对比度自动校验。无需手动折腾配置，复制一条命令或直接在终端运行 `kiseki` 按 Tab 浏览安装。'
                : 'Every community pack passes whitelist sandbox, naga shader, and WCAG contrast checks. Copy a one-liner below or press Tab inside `kiseki` to browse and install live.'}
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-3 font-k-mono text-xs">
            <button
              type="button"
              onClick={() => onOpenDocs('sdk')}
              className="px-4 py-2.5 border cursor-pointer transition-colors"
              style={{
                background: 'color-mix(in srgb, var(--k-surface0) 75%, transparent)',
                borderColor: 'color-mix(in srgb, var(--k-border) 55%, transparent)',
                color: 'var(--k-text-primary)',
                borderRadius: 'var(--k-radius)',
              }}
            >
              {isZh ? '制作指南 (theme init)' : 'Authoring Guide'}
            </button>
            <a
              href={`${GALLERY_REGISTRY_REPO}#readme`}
              target="_blank"
              rel="noopener noreferrer"
              className="px-4 py-2.5 border font-semibold transition-opacity hover:opacity-90"
              style={{
                background: 'var(--k-accent-primary)',
                borderColor: 'var(--k-accent-primary)',
                color: 'var(--k-text-inverse)',
                borderRadius: 'var(--k-radius)',
              }}
            >
              {isZh ? '+ 提交主题到市场 →' : '+ Submit Theme →'}
            </a>
          </div>
        </div>

        {/* Search + Filter + Sort bar */}
        <div className="mt-7 flex flex-col lg:flex-row lg:items-center justify-between gap-4">
          <div className="flex flex-wrap items-center gap-2.5 flex-1">
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={isZh ? '搜索主题名、短 ID、作者或标签...' : 'Search theme ID, name, author, or tag...'}
              className="w-full sm:w-72 px-3.5 py-2 border font-k-mono text-xs outline-none"
              style={{
                background: 'color-mix(in srgb, var(--k-mantle) 85%, transparent)',
                borderColor: 'color-mix(in srgb, var(--k-border) 55%, transparent)',
                color: 'var(--k-text-primary)',
                borderRadius: 'var(--k-radius)',
              }}
            />

            {/* Scope pills */}
            <div className="inline-flex border font-k-mono text-[0.7rem]" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 45%, transparent)', borderRadius: 'var(--k-radius)' }}>
              {(
                [
                  ['all', isZh ? '全部' : 'All'],
                  ['community', isZh ? '社区' : 'Community'],
                  ['official', isZh ? '官方' : 'Official'],
                ] as const
              ).map(([k, label]) => (
                <button
                  key={k}
                  type="button"
                  onClick={() => setScope(k)}
                  className="px-3 py-2 cursor-pointer border-0"
                  style={{
                    background: scope === k ? 'var(--k-surface1)' : 'transparent',
                    color: scope === k ? 'var(--k-accent-primary)' : 'var(--k-text-secondary)',
                  }}
                >
                  {label}
                </button>
              ))}
            </div>

            {/* Dark/Light toggle */}
            <div className="inline-flex border font-k-mono text-[0.7rem]" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 45%, transparent)', borderRadius: 'var(--k-radius)' }}>
              {(
                [
                  ['all', isZh ? '明暗不限' : 'Any'],
                  ['dark', 'Dark'],
                  ['light', 'Light'],
                ] as const
              ).map(([k, label]) => (
                <button
                  key={k}
                  type="button"
                  onClick={() => setAppearance(k)}
                  className="px-2.5 py-2 cursor-pointer border-0"
                  style={{
                    background: appearance === k ? 'var(--k-surface1)' : 'transparent',
                    color: appearance === k ? 'var(--k-accent-primary)' : 'var(--k-text-secondary)',
                  }}
                >
                  {label}
                </button>
              ))}
            </div>

            <button
              type="button"
              onClick={() => setOnlyShader((v) => !v)}
              className="px-3 py-2 border font-k-mono text-[0.7rem] cursor-pointer"
              style={{
                background: onlyShader ? 'var(--k-surface1)' : 'transparent',
                borderColor: onlyShader ? 'var(--k-accent-primary)' : 'color-mix(in srgb, var(--k-border) 45%, transparent)',
                color: onlyShader ? 'var(--k-accent-primary)' : 'var(--k-text-secondary)',
                borderRadius: 'var(--k-radius)',
              }}
            >
              ✦ Live Shader
            </button>

            <button
              type="button"
              onClick={() => setOnlyWallpaper((v) => !v)}
              className="px-3 py-2 border font-k-mono text-[0.7rem] cursor-pointer"
              style={{
                background: onlyWallpaper ? 'var(--k-surface1)' : 'transparent',
                borderColor: onlyWallpaper ? 'var(--k-accent-primary)' : 'color-mix(in srgb, var(--k-border) 45%, transparent)',
                color: onlyWallpaper ? 'var(--k-accent-primary)' : 'var(--k-text-secondary)',
                borderRadius: 'var(--k-radius)',
              }}
            >
              {isZh ? '🖼 壁纸' : '🖼 Wallpaper'}
            </button>
          </div>

          {/* Sort selector */}
          <div className="flex items-center gap-2 font-k-mono text-[0.7rem]">
            <span style={{ color: 'var(--k-text-muted)' }}>{isZh ? '排序:' : 'Sort:'}</span>
            <div className="inline-flex flex-wrap border" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 45%, transparent)', borderRadius: 'var(--k-radius)' }}>
              {sortOptions.map((opt) => (
                <button
                  key={opt.key}
                  type="button"
                  onClick={() => setSort(opt.key)}
                  className="px-2.5 py-2 cursor-pointer border-0"
                  style={{
                    background: sort === opt.key ? 'var(--k-surface1)' : 'transparent',
                    color: sort === opt.key ? 'var(--k-accent-primary)' : 'var(--k-text-secondary)',
                  }}
                >
                  {isZh ? opt.zh : opt.en}
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Status notice when community index is still empty or offline */}
        {!loading && communityThemes.length === 0 && (
          <div
            className="mt-6 px-4 py-3.5 border flex flex-wrap items-center justify-between gap-4 font-k-mono text-xs"
            style={{
              background: 'color-mix(in srgb, var(--k-surface0) 45%, transparent)',
              borderColor: 'color-mix(in srgb, var(--k-border) 45%, transparent)',
              color: 'var(--k-text-secondary)',
              borderRadius: 'var(--k-radius)',
            }}
          >
            <span>
              {offline
                ? isZh
                  ? '社区远程索引暂未就绪或当前离线，已优先展示 4 套官方内置主题。你也可随时通过 `kiseki theme install github:owner/repo` 直装任意仓库主题。'
                  : 'Remote community index is offline or not yet published — showing the 4 built-in themes. You can still install directly via `kiseki theme install github:owner/repo`.'
                : isZh
                  ? '社区市场刚刚开放！运行 `kiseki theme init my-theme` 创建你的主题，提交 1 个 YAML 文件 PR 即可成为首批收录创作者。'
                  : 'Kiseki Gallery just launched! Run `kiseki theme init my-theme` and open a single-YAML PR to be featured among the first community themes.'}
            </span>
            <a
              href={`${GALLERY_REGISTRY_REPO}#readme`}
              target="_blank"
              rel="noopener noreferrer"
              className="underline"
              style={{ color: 'var(--k-accent-primary)' }}
            >
              nerv-base/kiseki-themes →
            </a>
          </div>
        )}

        {/* Cards Grid */}
        {loading ? (
          <div className="mt-10 grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
            {[0, 1, 2].map((i) => (
              <div
                key={i}
                className="h-80 border animate-pulse"
                style={{
                  background: 'color-mix(in srgb, var(--k-surface0) 40%, transparent)',
                  borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)',
                }}
              />
            ))}
          </div>
        ) : visible.length === 0 ? (
          <div
            className="mt-12 py-16 text-center border font-k-mono text-sm"
            style={{
              borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)',
              color: 'var(--k-text-muted)',
            }}
          >
            {isZh ? '没有找到匹配的主题，试试清空搜索或切换筛选条件。' : 'No themes match the current filters.'}
          </div>
        ) : (
          <div className="mt-8 grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
            {visible.map((item) => {
              const cmd = installCommandFor(item);
              const isCopied = copiedId === item.id;
              return (
                <article
                  key={item.id}
                  onClick={() => setSelected(item)}
                  className="group border flex flex-col cursor-pointer transition-all hover:-translate-y-0.5"
                  style={{
                    background: 'color-mix(in srgb, var(--k-panel-bg) 82%, transparent)',
                    borderColor: 'color-mix(in srgb, var(--k-border) 45%, transparent)',
                    borderRadius: 'var(--k-radius)',
                  }}
                >
                  {/* Visual screenshot / live fallback */}
                  <div className="relative aspect-[16/10] w-full overflow-hidden border-b" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)' }}>
                    <CardPreview theme={item} />
                    <div className="absolute top-2.5 left-2.5 flex flex-wrap gap-1.5 font-k-mono text-[0.625rem]">
                      <span
                        className="px-2 py-0.5 border backdrop-blur-sm"
                        style={{
                          background: 'color-mix(in srgb, #000 62%, transparent)',
                          borderColor: item.builtin ? 'var(--k-accent-primary)' : 'rgba(255,255,255,0.2)',
                          color: item.builtin ? 'var(--k-accent-primary)' : '#F2F6FF',
                        }}
                      >
                        {item.builtin ? (isZh ? '官方内置' : 'BUILT-IN') : `v${item.version}`}
                      </span>
                      <span
                        className="px-2 py-0.5 backdrop-blur-sm uppercase"
                        style={{ background: 'color-mix(in srgb, #000 62%, transparent)', color: '#E2E8F0' }}
                      >
                        {item.appearance}
                      </span>
                      {item.capabilities.hasShader && (
                        <span
                          className="px-2 py-0.5 backdrop-blur-sm"
                          style={{ background: 'color-mix(in srgb, #000 62%, transparent)', color: '#FFE5A3' }}
                        >
                          SHADER
                        </span>
                      )}
                    </div>

                    {!item.builtin && (
                      <div
                        className="absolute bottom-2.5 right-2.5 px-2 py-0.5 font-k-mono text-[0.6875rem] backdrop-blur-sm flex items-center gap-2.5"
                        style={{ background: 'color-mix(in srgb, #000 68%, transparent)', color: '#F8FAFC' }}
                      >
                        <span>★ {formatCompactCount(item.metrics.stars)}</span>
                        <span>↓ {formatCompactCount(item.metrics.downloads)}</span>
                      </div>
                    )}
                  </div>

                  {/* Card body */}
                  <div className="p-4 flex-1 flex flex-col justify-between gap-4">
                    <div>
                      <div className="flex items-baseline justify-between gap-2">
                        <h2 className="font-k-display text-base tracking-wide truncate" style={{ color: 'var(--k-text-primary)' }}>
                          {item.name}
                        </h2>
                        <span className="font-k-mono text-[0.6875rem] shrink-0" style={{ color: 'var(--k-accent-primary)' }}>
                          {item.id}
                        </span>
                      </div>
                      <p className="mt-1.5 text-xs leading-relaxed line-clamp-2" style={{ color: 'var(--k-text-secondary)' }}>
                        {item.subtitle || item.repo}
                      </p>
                      <div className="mt-3 flex items-center justify-between gap-2 font-k-mono text-[0.6875rem]" style={{ color: 'var(--k-text-muted)' }}>
                        <span className="truncate">
                          {item.authors.map((a) => a.name).join(', ') || item.repo.split('/')[0]}
                        </span>
                        <span className="shrink-0">
                          WCAG {item.capabilities.wcagGrade || 'AA'} · {item.license || 'MIT'}
                        </span>
                      </div>
                    </div>

                    {/* Quick install bar */}
                    <div
                      className="flex items-center justify-between gap-2 px-2.5 py-2 border font-k-mono text-[0.6875rem]"
                      style={{
                        background: 'color-mix(in srgb, var(--k-mantle) 85%, transparent)',
                        borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)',
                      }}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <code className="truncate" style={{ color: 'var(--k-text-secondary)' }}>
                        {item.builtin ? `kiseki use ${item.id}` : `kiseki theme install ${item.id}`}
                      </code>
                      <button
                        type="button"
                        onClick={() => copyCommand(item.id, cmd)}
                        className="shrink-0 px-2 py-0.5 cursor-pointer border-0 font-semibold"
                        style={{
                          background: isCopied ? 'var(--k-success)' : 'var(--k-surface1)',
                          color: isCopied ? 'var(--k-text-inverse)' : 'var(--k-accent-primary)',
                        }}
                      >
                        {isCopied ? '✓' : isZh ? '复制' : 'COPY'}
                      </button>
                    </div>
                  </div>
                </article>
              );
            })}
          </div>
        )}

        {/* Footer links */}
        <div
          className="mt-14 pt-6 border-t flex flex-wrap items-center justify-between gap-4 font-k-mono text-xs"
          style={{
            borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)',
            color: 'var(--k-text-muted)',
          }}
        >
          <button
            type="button"
            onClick={() => onNavigate('home')}
            className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
            style={{ color: 'var(--k-text-secondary)' }}
          >
            {isZh ? '← 返回首页试玩' : '← Back to Live Playground'}
          </button>
          <button
            type="button"
            onClick={() => onNavigate('download')}
            className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
            style={{ color: 'var(--k-accent-primary)' }}
          >
            {isZh ? '还没有安装 kiseki？前往下载 →' : 'Need kiseki CLI? Download →'}
          </button>
        </div>
      </div>

      {/* Detail Modal */}
      {selected && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center p-4 sm:p-6 backdrop-blur-md"
          style={{ background: 'rgba(4, 7, 15, 0.78)' }}
          onClick={() => setSelected(null)}
        >
          <div
            className="w-full max-w-3xl max-h-[90vh] overflow-y-auto border shadow-2xl"
            style={{
              background: 'var(--k-base)',
              borderColor: 'var(--k-border)',
              borderRadius: 'var(--k-radius)',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="relative aspect-[16/9] w-full border-b" style={{ borderColor: 'var(--k-border)' }}>
              <CardPreview theme={selected} tall />
              <button
                type="button"
                onClick={() => setSelected(null)}
                aria-label="Close"
                className="absolute top-3 right-3 px-2.5 py-1 border font-k-mono text-xs cursor-pointer"
                style={{
                  background: 'rgba(0,0,0,0.72)',
                  borderColor: 'rgba(255,255,255,0.25)',
                  color: '#fff',
                }}
              >
                ESC ✕
              </button>
            </div>

            <div className="p-6 space-y-5">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div>
                  <div className="flex items-center gap-2.5">
                    <h2 className="font-k-display text-2xl tracking-wide" style={{ color: 'var(--k-text-primary)' }}>
                      {selected.name}
                    </h2>
                    <span className="font-k-mono text-xs px-2 py-0.5 border" style={{ borderColor: 'var(--k-border)', color: 'var(--k-accent-primary)' }}>
                      {selected.id}
                    </span>
                  </div>
                  <p className="mt-1.5 text-sm" style={{ color: 'var(--k-text-secondary)' }}>
                    {selected.subtitle}
                  </p>
                </div>

                {selected.builtin && (
                  <button
                    type="button"
                    onClick={() => {
                      onTryOfficialTheme(selected.id as ThemeId);
                      setSelected(null);
                    }}
                    className="px-4 py-2 font-k-mono text-xs font-semibold cursor-pointer border-0"
                    style={{
                      background: 'var(--k-accent-primary)',
                      color: 'var(--k-text-inverse)',
                    }}
                  >
                    {isZh ? '在首页在线试玩 →' : 'Try Live in Playground →'}
                  </button>
                )}
              </div>

              {/* Install snippet */}
              <div
                className="p-3.5 border flex items-center justify-between gap-3 font-k-mono text-xs"
                style={{
                  background: 'var(--k-mantle)',
                  borderColor: 'color-mix(in srgb, var(--k-border) 50%, transparent)',
                }}
              >
                <code className="overflow-x-auto" style={{ color: 'var(--k-text-primary)' }}>
                  {installCommandFor(selected)}
                </code>
                <button
                  type="button"
                  onClick={() => copyCommand(`modal-${selected.id}`, installCommandFor(selected))}
                  className="shrink-0 px-3 py-1.5 border-0 cursor-pointer font-semibold"
                  style={{
                    background: copiedId === `modal-${selected.id}` ? 'var(--k-success)' : 'var(--k-accent-primary)',
                    color: 'var(--k-text-inverse)',
                  }}
                >
                  {copiedId === `modal-${selected.id}` ? (isZh ? '已复制 ✓' : 'COPIED ✓') : isZh ? '复制命令' : 'COPY'}
                </button>
              </div>

              {/* Capabilities & Metrics */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 font-k-mono text-xs">
                <div className="p-3 border" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 40%, transparent)', background: 'var(--k-mantle)' }}>
                  <div style={{ color: 'var(--k-text-muted)' }}>WCAG</div>
                  <div className="mt-1 font-semibold" style={{ color: 'var(--k-text-primary)' }}>
                    {selected.capabilities.wcagGrade || 'AA'} · {selected.appearance}
                  </div>
                </div>
                <div className="p-3 border" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 40%, transparent)', background: 'var(--k-mantle)' }}>
                  <div style={{ color: 'var(--k-text-muted)' }}>Shader</div>
                  <div className="mt-1 font-semibold" style={{ color: 'var(--k-text-primary)' }}>
                    {selected.capabilities.hasShader
                      ? selected.capabilities.shaderProfiles.join('/') || 'GLSL/HLSL'
                      : isZh
                        ? '无（纯静态）'
                        : 'Static'}
                  </div>
                </div>
                <div className="p-3 border" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 40%, transparent)', background: 'var(--k-mantle)' }}>
                  <div style={{ color: 'var(--k-text-muted)' }}>{isZh ? '壁纸' : 'Wallpaper'}</div>
                  <div className="mt-1 font-semibold" style={{ color: 'var(--k-text-primary)' }}>
                    {selected.capabilities.hasWallpaper ? (isZh ? '包含' : 'Included') : isZh ? '无' : 'None'}
                  </div>
                </div>
                <div className="p-3 border" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 40%, transparent)', background: 'var(--k-mantle)' }}>
                  <div style={{ color: 'var(--k-text-muted)' }}>{isZh ? '热度' : 'Popularity'}</div>
                  <div className="mt-1 font-semibold" style={{ color: 'var(--k-text-primary)' }}>
                    {selected.builtin
                      ? isZh
                        ? '官方内置'
                        : 'Built-in'
                      : `★ ${selected.metrics.stars} · ↓ ${selected.metrics.downloads}`}
                  </div>
                </div>
              </div>

              {/* Palette swatches */}
              <div>
                <div className="font-k-mono text-[0.6875rem] uppercase tracking-wider mb-2" style={{ color: 'var(--k-text-muted)' }}>
                  {isZh ? '核心色板（点击复制色值）' : 'Core Palette (Click to copy)'}
                </div>
                <div className="grid grid-cols-4 sm:grid-cols-8 gap-2">
                  {(
                    [
                      ['base', selected.preview.palette.base],
                      ['surface', selected.preview.palette.surface0],
                      ['text', selected.preview.palette.textPrimary],
                      ['accent', selected.preview.palette.accentPrimary],
                      ['accent2', selected.preview.palette.accentSecondary],
                      ['keyword', selected.preview.palette.keyword],
                      ['string', selected.preview.palette.string],
                      ['func', selected.preview.palette.function],
                    ] as const
                  ).map(([label, hex]) => (
                    <button
                      key={label}
                      type="button"
                      onClick={() => copyCommand(`hex-${label}`, hex)}
                      className="p-1.5 border text-left cursor-pointer"
                      style={{
                        borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)',
                        background: 'var(--k-mantle)',
                      }}
                    >
                      <span className="block w-full h-6 mb-1" style={{ background: hex }} />
                      <span className="block font-k-mono text-[0.6rem] truncate" style={{ color: 'var(--k-text-secondary)' }}>
                        {copiedId === `hex-${label}` ? '✓' : hex}
                      </span>
                    </button>
                  ))}
                </div>
              </div>

              {/* Lazy-loaded release notes */}
              {!selected.builtin && updatesMap?.themes[selected.id]?.releaseNotes && (
                <div
                  className="p-3.5 border font-k-mono text-xs whitespace-pre-wrap leading-relaxed"
                  style={{
                    background: 'var(--k-mantle)',
                    borderColor: 'color-mix(in srgb, var(--k-border) 40%, transparent)',
                    color: 'var(--k-text-secondary)',
                  }}
                >
                  <div className="mb-1.5 font-semibold" style={{ color: 'var(--k-text-primary)' }}>
                    {isZh ? '最近更新日志' : 'Latest Release Notes'}
                  </div>
                  {updatesMap.themes[selected.id]!.releaseNotes}
                </div>
              )}

              {/* Provenance, GitHub, and Notice-and-Takedown */}
              <div
                className="pt-4 border-t flex flex-wrap items-center justify-between gap-3 font-k-mono text-xs"
                style={{
                  borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)',
                  color: 'var(--k-text-muted)',
                }}
              >
                <div className="flex flex-wrap items-center gap-4">
                  <a
                    href={`https://github.com/${selected.repo}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="hover:underline"
                    style={{ color: 'var(--k-accent-primary)' }}
                  >
                    GitHub ({selected.repo}) ↗
                  </a>
                  <span>License: {selected.license || 'MIT'}</span>
                </div>
                {!selected.builtin && (
                  <a
                    href={`${GALLERY_REGISTRY_REPO}/issues/new?template=takedown.yml&title=${encodeURIComponent(
                      `[Takedown] ${selected.id}`,
                    )}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="hover:underline"
                    style={{ color: 'var(--k-text-muted)' }}
                  >
                    {isZh ? '版权/违规下架反馈' : 'Report / DMCA Takedown'}
                  </a>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </section>
  );
};
