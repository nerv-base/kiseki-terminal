import { useEffect, useMemo, useRef, useState, type FC } from 'react';
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

export type GalleryCanvasMode = 'dark' | 'light';

interface GalleryHubProps {
  lang: Lang;
  canvasMode: GalleryCanvasMode;
  onToggleCanvasMode: () => void;
  onTryOfficialTheme: (id: ThemeId) => void;
  onOpenDocs: (chapter: DocChapterId) => void;
  onNavigate: (target: 'home' | 'download') => void;
}

/**
 * Fallback plate for community themes submitted without a screenshot thumbnail.
 * Pure, sharp token & wallpaper preview — zero fake macOS traffic light dots.
 */
const TokenFallbackPlate: FC<{ theme: GalleryThemeEntry }> = ({ theme }) => {
  const pal = theme.preview.palette;
  const wallpaper = theme.preview.wallpaperUrl;
  return (
    <div
      className="relative w-full h-full flex flex-col justify-between p-5 select-none overflow-hidden"
      style={{ background: pal.base || '#0B0F17', color: pal.textPrimary || '#EFF3FA' }}
    >
      {wallpaper && (
        <>
          <img
            src={wallpaper}
            alt=""
            aria-hidden="true"
            loading="lazy"
            className="kg-plate-img absolute inset-0 w-full h-full object-cover object-right opacity-55 pointer-events-none"
          />
          <div
            className="absolute inset-0 pointer-events-none"
            style={{
              background: `linear-gradient(95deg, ${pal.base} 0%, color-mix(in srgb, ${pal.base} 84%, transparent) 52%, transparent 100%)`,
            }}
          />
        </>
      )}
      <div className="relative z-10 flex items-center justify-between kg-mono text-[11px] opacity-65">
        <span>{theme.id}</span>
        <span>WCAG {theme.capabilities.wcagGrade || 'AA'}</span>
      </div>
      <div className="relative z-10 kg-mono text-[12px] leading-relaxed space-y-1">
        <div>
          <span style={{ color: pal.accentPrimary }}>$</span> kiseki theme install{' '}
          <span style={{ color: pal.string }}>{theme.id}</span>
        </div>
        <div className="opacity-75">
          <span style={{ color: pal.keyword }}>palette</span> ·{' '}
          <span style={{ color: pal.accentSecondary }}>{theme.appearance}</span>
        </div>
      </div>
    </div>
  );
};

/** Real terminal screenshot plate (native 16:11 ratio preserves 100% of Fastfetch + Prompt + Wallpaper). */
const ThemeScreenshotPlate: FC<{ theme: GalleryThemeEntry }> = ({ theme }) => {
  const [imgFailed, setImgFailed] = useState(false);
  const thumb = theme.preview.thumbUrl;

  useEffect(() => {
    setImgFailed(false);
  }, [thumb]);

  return (
    <div className="kg-plate aspect-[16/11] w-full">
      <span className="kg-corner-tl" aria-hidden="true" />
      <span className="kg-corner-br" aria-hidden="true" />
      {thumb && !imgFailed ? (
        <img
          src={thumb}
          alt={`${theme.name} 真实终端实机截图`}
          loading="lazy"
          onError={() => setImgFailed(true)}
          className="kg-plate-img w-full h-full object-cover object-center block"
        />
      ) : (
        <TokenFallbackPlate theme={theme} />
      )}
    </div>
  );
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

  const [query, setQuery] = useState('');
  const [appearance, setAppearance] = useState<'all' | 'dark' | 'light'>('all');
  const [onlyShader, setOnlyShader] = useState(false);
  const [onlyWallpaper, setOnlyWallpaper] = useState(false);
  const [sort, setSort] = useState<GallerySortKey>('trending');
  const [sortOpen, setSortOpen] = useState(false);
  const sortMenuRef = useRef<HTMLDivElement | null>(null);

  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [selected, setSelected] = useState<GalleryThemeEntry | null>(null);
  const [modalMedia, setModalMedia] = useState<'screenshot' | 'wallpaper'>('screenshot');
  const [modalDeskLight, setModalDeskLight] = useState(false);
  const [hoveredSwatch, setHoveredSwatch] = useState<{ label: string; hex: string } | null>(null);

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

  useEffect(() => {
    if (!selected) return;
    setModalMedia('screenshot');
    setModalDeskLight(selected.appearance === 'light');
    setHoveredSwatch(null);
    if (!selected.builtin && !updatesMap && sourceUrl) {
      let active = true;
      fetchGalleryUpdates(sourceUrl).then((payload) => {
        if (active && payload) setUpdatesMap(payload);
      });
      return () => {
        active = false;
      };
    }
  }, [selected, updatesMap, sourceUrl]);

  useEffect(() => {
    if (!selected) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSelected(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selected]);

  useEffect(() => {
    if (!sortOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (sortMenuRef.current && !sortMenuRef.current.contains(e.target as Node)) {
        setSortOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSortOpen(false);
    };
    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onPointerDown);
      window.removeEventListener('keydown', onKey);
    };
  }, [sortOpen]);

  const sortOptions: Array<[GallerySortKey, string]> = [
    ['trending', isZh ? '综合热门' : 'Trending'],
    ['downloads', isZh ? '最多下载' : 'Most downloaded'],
    ['stars', isZh ? '最多 Star' : 'Most starred'],
    ['updated', isZh ? '最近更新' : 'Recently updated'],
    ['newest', isZh ? '最新上架' : 'Newest'],
  ];
  const currentSortLabel = sortOptions.find(([k]) => k === sort)?.[1] ?? sortOptions[0]![1];

  const allEntries = useMemo(
    () => [...communityThemes, ...OFFICIAL_GALLERY_THEMES],
    [communityThemes],
  );

  const visible = useMemo(
    () =>
      filterAndSortGallery(allEntries, {
        scope: 'all',
        query,
        appearance,
        onlyShader,
        onlyWallpaper,
        sort,
      }),
    [allEntries, query, appearance, onlyShader, onlyWallpaper, sort],
  );

  const copyText = (key: string, text: string) => {
    const done = () => {
      setCopiedKey(key);
      window.setTimeout(() => setCopiedKey((cur) => (cur === key ? null : cur)), 1500);
    };
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(done);
    } else {
      done();
    }
  };

  const installCommandFor = (item: GalleryThemeEntry) =>
    item.builtin
      ? `kiseki use ${item.id}`
      : `kiseki theme install ${item.id} && kiseki use ${item.id}`;

  const composeCommandFor = (item: GalleryThemeEntry) =>
    `kiseki compose --atmosphere ${item.id}`;

  const shortPoeticTag = (item: GalleryThemeEntry) => {
    const first = (item.subtitle || '').split('·')[0]?.trim();
    return first || (isZh ? '官方内置' : 'Built-in');
  };

  const isDense = visible.length >= 5;

  return (
    <main
      id="k-view-gallery"
      className="kg-shell relative pb-24 overflow-x-clip"
    >
      {/* Tactile darkroom film grain */}
      <div className="kg-grain" aria-hidden="true" />

      {/* ACG Key-Visual Hero Band — bespoke Kiseki mascot integrated seamlessly into the right backdrop */}
      <section className="relative pt-[60px]">
        <div
          className="kg-container relative min-h-[260px] sm:min-h-[320px] lg:min-h-[356px] flex items-end pb-9 sm:pb-11 overflow-hidden"
          data-wide={isDense ? 'true' : undefined}
        >
          {/* Bespoke Kiseki Mascot (看板娘) — right-anchored, zero frame, clean headroom below header */}
          <div
            className="pointer-events-none select-none absolute top-3 sm:top-4 bottom-0 right-0 sm:right-4 lg:right-8 w-[340px] sm:w-[540px] lg:w-[660px] xl:w-[720px] flex justify-end items-end overflow-hidden"
            aria-hidden="true"
          >
            <img
              src="/gallery-mascot.webp"
              alt=""
              className="kg-mascot-img h-[238px] sm:h-[296px] lg:h-[332px] xl:h-[340px] w-auto max-w-none object-contain object-right-bottom opacity-60 sm:opacity-95"
            />
          </div>

          {/* Mobile readability scrim over mascot's left edge */}
          <div
            className="sm:hidden pointer-events-none absolute inset-0"
            style={{
              background:
                'linear-gradient(90deg, #070A0F 0%, rgba(7,10,15,0.88) 48%, rgba(7,10,15,0.22) 100%)',
            }}
            aria-hidden="true"
          />

          {/* Left Editorial ACG Lockup */}
          <div className="relative z-10 flex items-start gap-4 sm:gap-5 max-w-[540px] pt-8 sm:pt-10">
            {/* Minimal Japanese vertical hanko seal (matches Kiseki's signature poster language) */}
            <div className="hidden sm:flex flex-col items-center gap-2.5 pt-1 select-none shrink-0">
              <span
                className="kg-vertical-stamp kg-serif text-[11px] px-1 py-1.5 border"
                style={{
                  borderColor: 'rgba(240, 155, 184, 0.55)',
                  color: 'var(--kg-accent-rose)',
                }}
              >
                主題
              </span>
              <span
                className="w-px h-10 opacity-35"
                style={{ background: 'var(--kg-accent)' }}
                aria-hidden="true"
              />
            </div>

            <div className="min-w-0">
              <div className="flex items-center gap-2.5">
                <span
                  className="w-5 h-px shrink-0 opacity-75"
                  style={{ background: 'var(--kg-accent)' }}
                  aria-hidden="true"
                />
                <span
                  className="kg-serif text-[12px] tracking-[0.26em]"
                  style={{ color: 'var(--kg-accent-rose)' }}
                >
                  キセキ・テーマ市場
                </span>
                <span
                  className="kg-mono text-[11px] tracking-[0.16em] uppercase opacity-55"
                  style={{ color: 'var(--kg-ink-secondary)' }}
                >
                  ARCHIVE
                </span>
              </div>

              <div className="mt-2.5 flex items-baseline gap-3">
                <h1
                  className="kg-serif text-[34px] sm:text-[44px] font-normal leading-[1.06] tracking-[0.04em] m-0"
                  style={{ color: 'var(--kg-ink)' }}
                >
                  {isZh ? '奇蹟主题市场' : 'Kiseki Gallery'}
                </h1>
                <span
                  className="kg-mono text-[12px] tracking-[0.1em]"
                  style={{ color: 'var(--kg-ink-muted)' }}
                >
                  {String(visible.length).padStart(2, '0')}
                </span>
              </div>

              <div
                className="mt-3 h-px w-full max-w-[340px]"
                style={{
                  background:
                    'linear-gradient(90deg, rgba(120, 214, 238, 0.55), transparent)',
                }}
                aria-hidden="true"
              />

              <p
                className="mt-3 m-0 leading-relaxed"
                style={{ fontSize: 'var(--kg-text-base)', color: 'var(--kg-ink-secondary)' }}
              >
                {isZh
                  ? '全实机截图所见即所得 · 5 层声明式主题包，支持按层混搭与一键回滚。'
                  : 'Authentic real-terminal captures · 5-layer declarative packs with layer mixing and instant rollback.'}
              </p>

              <div className="mt-5 flex flex-wrap items-center gap-5">
                <a
                  href={`${GALLERY_REGISTRY_REPO}#readme`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="kg-cta-sharp"
                >
                  <span>{isZh ? '提交社区主题' : 'Submit a theme'}</span>
                  <span style={{ color: 'var(--kg-accent)' }} aria-hidden="true">→</span>
                </a>

                <button
                  type="button"
                  onClick={() => onOpenDocs('sdk')}
                  className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                  style={{ fontSize: 'var(--kg-text-sm)', color: 'var(--kg-ink-secondary)' }}
                >
                  {isZh ? '主题开发文档' : 'Authoring guide'}
                </button>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Main Directory Content */}
      <div className="kg-container relative z-10" data-wide={isDense ? 'true' : undefined}>
        {/* Single Frameless Architectural Filter Hairline */}
        <div
          className="flex flex-col md:flex-row md:items-end justify-between gap-4 border-b"
          style={{ borderColor: 'var(--kg-hairline)' }}
        >
          <div className="flex items-center gap-5 sm:gap-6 overflow-x-auto">
            {(
              [
                ['all', isZh ? '全部' : 'All'],
                ['dark', isZh ? '暗色' : 'Dark'],
                ['light', isZh ? '浅色' : 'Light'],
              ] as const
            ).map(([key, label]) => (
              <button
                key={key}
                type="button"
                className="kg-tab"
                aria-pressed={appearance === key}
                onClick={() => setAppearance(key)}
              >
                {label}
              </button>
            ))}

            <span
              className="h-3.5 w-px mb-3 shrink-0"
              style={{ background: 'var(--kg-hairline-strong)' }}
              aria-hidden="true"
            />

            <button
              type="button"
              className="kg-toggle-link"
              aria-pressed={onlyShader}
              onClick={() => setOnlyShader((v) => !v)}
            >
              <span>{isZh ? '动态 Shader' : 'Live shader'}</span>
            </button>

            <button
              type="button"
              className="kg-toggle-link"
              aria-pressed={onlyWallpaper}
              onClick={() => setOnlyWallpaper((v) => !v)}
            >
              <span>{isZh ? '含壁纸' : 'Wallpaper'}</span>
            </button>
          </div>

          <div className="flex items-center justify-between md:justify-end gap-5 pb-2 shrink-0">
            <div className="relative w-44 sm:w-52">
              <span
                className="kg-mono text-[12px] absolute left-0 top-1/2 -translate-y-1/2 pointer-events-none select-none"
                style={{ color: 'var(--kg-accent)' }}
                aria-hidden="true"
              >
                /
              </span>
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={isZh ? '搜索主题或作者' : 'Search themes'}
                className="kg-search-input"
              />
            </div>

            <span
              className="h-3.5 w-px shrink-0"
              style={{ background: 'var(--kg-hairline-strong)' }}
              aria-hidden="true"
            />

            <div ref={sortMenuRef} className="relative">
              <button
                type="button"
                aria-haspopup="listbox"
                aria-expanded={sortOpen}
                onClick={() => setSortOpen((v) => !v)}
                className="inline-flex items-center gap-1.5 bg-transparent border-0 p-0 cursor-pointer font-medium whitespace-nowrap"
                style={{ fontSize: 'var(--kg-text-sm)', color: 'var(--kg-ink-secondary)' }}
              >
                <span>{currentSortLabel}</span>
                <svg
                  viewBox="0 0 12 12"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  className="w-3 h-3 opacity-70"
                  aria-hidden="true"
                >
                  <path d="M3 4.5L6 7.5L9 4.5" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </button>

              {sortOpen && (
                <div
                  role="listbox"
                  aria-label={isZh ? '排序方式' : 'Sort order'}
                  className="kg-sort-menu absolute right-0 top-full mt-2 min-w-[140px] py-1.5 z-30"
                >
                  {sortOptions.map(([key, label]) => (
                    <button
                      key={key}
                      type="button"
                      role="option"
                      aria-selected={sort === key}
                      onClick={() => {
                        setSort(key);
                        setSortOpen(false);
                      }}
                      className="w-full text-left px-3.5 py-1.5 bg-transparent border-0 cursor-pointer flex items-center justify-between gap-3"
                      style={{
                        fontSize: 'var(--kg-text-sm)',
                        color: sort === key ? 'var(--kg-accent)' : 'var(--kg-ink-secondary)',
                        fontWeight: sort === key ? 600 : 400,
                      }}
                    >
                      <span>{label}</span>
                      {sort === key && <span aria-hidden="true">✓</span>}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Real Terminal Screenshot Grid — 1 / 2 / 3 / 4 / 5 Responsive Columns */}
        {loading ? (
          <div className="mt-8 kg-grid">
            {[0, 1, 2, 3].map((i) => (
              <div key={i} className="space-y-2.5">
                <div
                  className="aspect-[16/11] animate-pulse"
                  style={{ background: 'var(--kg-surface)' }}
                />
                <div className="h-3.5 w-32 animate-pulse" style={{ background: 'var(--kg-surface)' }} />
              </div>
            ))}
          </div>
        ) : visible.length === 0 ? (
          <div
            className="mt-12 py-24 text-center"
            style={{ fontSize: 'var(--kg-text-base)', color: 'var(--kg-ink-muted)' }}
          >
            {isZh ? '没有找到符合条件的主题。' : 'No themes match the current filters.'}
          </div>
        ) : (
          <div className="mt-8 kg-grid" data-dense={isDense ? 'true' : undefined}>
            {visible.map((item, idx) => {
              const indexLabel = String(idx + 1).padStart(2, '0');
              const cmd = installCommandFor(item);
              const isCopied = copiedKey === `tile-${item.id}`;
              const author = item.authors.map((a) => a.name).join(', ') || item.repo.split('/')[0];
              const dots = [
                item.preview.palette.accentPrimary,
                item.preview.palette.accentSecondary,
                item.preview.palette.keyword,
                item.preview.palette.string,
              ];
              const metaParts: string[] = [];
              if (item.builtin) {
                metaParts.push(shortPoeticTag(item));
                if (item.capabilities.hasShader) metaParts.push('Shader');
              } else {
                metaParts.push(author);
                metaParts.push(`★ ${formatCompactCount(item.metrics.stars)}`);
                metaParts.push(`↓ ${formatCompactCount(item.metrics.downloads)}`);
              }

              return (
                <article
                  key={item.id}
                  tabIndex={0}
                  onClick={() => setSelected(item)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      setSelected(item);
                    }
                  }}
                  className="kg-tile group flex flex-col"
                >
                  <ThemeScreenshotPlate theme={item} />

                  {/* Bare typography directly on the twilight canvas — zero card footer box */}
                  <div className="pt-3 px-0.5 flex flex-col gap-1">
                    <div className="flex items-baseline justify-between gap-2.5">
                      <div className="flex items-baseline gap-2 min-w-0">
                        <span
                          className="kg-mono text-[11px] shrink-0"
                          style={{ color: 'var(--kg-ink-muted)' }}
                        >
                          {indexLabel}
                        </span>
                        <h2
                          className="font-medium truncate m-0"
                          style={{ fontSize: 'var(--kg-text-lg)', color: 'var(--kg-ink)' }}
                        >
                          {item.name}
                        </h2>
                      </div>

                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          copyText(`tile-${item.id}`, cmd);
                        }}
                        className="kg-tile-action shrink-0 bg-transparent border-0 p-0 cursor-pointer font-medium hover:underline"
                        style={{
                          fontSize: 'var(--kg-text-sm)',
                          color: isCopied ? 'var(--kg-success)' : 'var(--kg-accent)',
                        }}
                      >
                        {isCopied ? (isZh ? '已复制' : 'Copied') : isZh ? '复制命令' : 'Copy'}
                      </button>
                    </div>

                    <div className="flex items-center justify-between gap-2 pl-[22px]">
                      <span
                        className="truncate kg-tabular"
                        style={{ fontSize: 'var(--kg-text-sm)', color: 'var(--kg-ink-muted)' }}
                      >
                        {metaParts.join(' · ')}
                      </span>
                      <div className="flex items-center gap-[5px] shrink-0" aria-hidden="true">
                        {dots.map((hex, i) => (
                          <span
                            key={`${hex}-${i}`}
                            className="w-[6px] h-[6px] rounded-full inline-block"
                            style={{ background: hex, opacity: 0.88 }}
                          />
                        ))}
                      </div>
                    </div>
                  </div>
                </article>
              );
            })}
          </div>
        )}

        {/* Quiet single-line community contribution note */}
        {!loading && communityThemes.length === 0 && (
          <p
            className="mt-12 m-0 flex flex-wrap items-center gap-x-2 gap-y-1"
            style={{ fontSize: 'var(--kg-text-sm)', color: 'var(--kg-ink-muted)' }}
          >
            <span>
              {offline
                ? isZh
                  ? '社区索引暂不可达，当前展示 4 套官方实机主题；你仍可用 kiseki theme install github:owner/repo 直接安装任意仓库。'
                  : 'Community index is temporarily offline. Direct installs via kiseki theme install github:owner/repo remain available.'
                : isZh
                  ? '社区投稿已开放 — 运行 kiseki theme init 生成主题脚手架，向 nerv-base/kiseki-themes 提交 1 个 YAML 即可上架。'
                  : 'Community submissions are open — run kiseki theme init and submit a single YAML file to nerv-base/kiseki-themes.'}
            </span>
            <a
              href={`${GALLERY_REGISTRY_REPO}#readme`}
              target="_blank"
              rel="noopener noreferrer"
              className="font-medium no-underline hover:underline"
              style={{ color: 'var(--kg-accent)' }}
            >
              {isZh ? '前往提交仓库 ↗' : 'Submit on GitHub ↗'}
            </a>
          </p>
        )}

        {/* Clean Minimal Footer */}
        <footer
          className="mt-12 pt-6 border-t flex flex-wrap items-center justify-between gap-4"
          style={{
            fontSize: 'var(--kg-text-sm)',
            borderColor: 'var(--kg-hairline)',
            color: 'var(--kg-ink-muted)',
          }}
        >
          <button
            type="button"
            onClick={() => onNavigate('home')}
            className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
            style={{ color: 'var(--kg-ink-secondary)' }}
          >
            {isZh ? '← 返回首页' : '← Back to Home'}
          </button>
          <div className="flex items-center gap-6">
            <button
              type="button"
              onClick={() => onOpenDocs('sdk')}
              className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
              style={{ color: 'var(--kg-ink-secondary)' }}
            >
              {isZh ? '主题开发文档' : 'Theme SDK docs'}
            </button>
            <button
              type="button"
              onClick={() => onNavigate('download')}
              className="bg-transparent border-0 p-0 cursor-pointer font-medium hover:underline"
              style={{ color: 'var(--kg-accent)' }}
            >
              {isZh ? '下载 Kiseki CLI →' : 'Download Kiseki CLI →'}
            </button>
          </div>
        </footer>
      </div>

      {/* Sharp Split-Studio Detail Lightbox (z-[60] cleanly above header) */}
      {selected && (
        <div
          className="fixed inset-0 z-[60] flex items-center justify-center p-4 sm:p-8 backdrop-blur-xl"
          style={{ background: 'rgba(4, 6, 10, 0.84)' }}
          onClick={() => setSelected(null)}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label={selected.name}
            className="w-full max-w-[1200px] max-h-[calc(100vh-2rem)] sm:max-h-[calc(100vh-4rem)] rounded-[2px] overflow-y-auto lg:overflow-hidden grid grid-cols-1 lg:grid-cols-12"
            style={{
              background: 'var(--kg-modal-bg)',
              boxShadow: 'var(--kg-modal-shadow)',
              border: '1px solid var(--kg-hairline-strong)',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            {/* Left 7 cols: Full-Resolution Real Terminal Capture / Pure Wallpaper */}
            <div
              className="lg:col-span-7 flex flex-col justify-between p-5 sm:p-8"
              style={{
                background: modalDeskLight ? '#EAECE8' : '#05070B',
              }}
            >
              <div className="flex-1 min-h-[240px] sm:min-h-[380px] flex items-center justify-center">
                {modalMedia === 'wallpaper' && selected.preview.wallpaperUrl ? (
                  <img
                    src={selected.preview.wallpaperUrl}
                    alt={`${selected.name} wallpaper artwork`}
                    className="w-full max-h-[520px] object-contain shadow-2xl block"
                    style={{ borderRadius: '2px' }}
                  />
                ) : selected.preview.thumbUrl ? (
                  <img
                    src={selected.preview.thumbUrl}
                    alt={`${selected.name} terminal screenshot`}
                    className="w-full max-h-[520px] object-contain shadow-2xl block"
                    style={{ borderRadius: '2px' }}
                  />
                ) : (
                  <div className="w-full aspect-[16/11]">
                    <TokenFallbackPlate theme={selected} />
                  </div>
                )}
              </div>

              {/* Quiet frameless stage switches along bottom */}
              <div
                className="pt-5 flex flex-wrap items-center justify-between gap-4"
                style={{
                  fontSize: 'var(--kg-text-sm)',
                  color: modalDeskLight ? '#58606E' : '#8590A6',
                }}
              >
                <div className="flex items-center gap-5">
                  <button
                    type="button"
                    onClick={() => setModalMedia('screenshot')}
                    className="bg-transparent border-0 p-0 cursor-pointer font-medium"
                    style={{
                      color:
                        modalMedia === 'screenshot'
                          ? modalDeskLight
                            ? '#0B0F17'
                            : '#EFF3FA'
                          : undefined,
                    }}
                  >
                    {isZh ? '实机终端截图' : 'Terminal capture'}
                  </button>
                  {selected.preview.wallpaperUrl && (
                    <button
                      type="button"
                      onClick={() => setModalMedia('wallpaper')}
                      className="bg-transparent border-0 p-0 cursor-pointer font-medium"
                      style={{
                        color:
                          modalMedia === 'wallpaper'
                            ? modalDeskLight
                              ? '#0B0F17'
                              : '#EFF3FA'
                            : undefined,
                      }}
                    >
                      {isZh ? '壁纸原画' : 'Wallpaper art'}
                    </button>
                  )}
                </div>

                <button
                  type="button"
                  onClick={() => setModalDeskLight((v) => !v)}
                  className="bg-transparent border-0 p-0 cursor-pointer font-medium"
                  style={{ color: modalDeskLight ? '#0B0F17' : '#EFF3FA' }}
                >
                  {modalDeskLight ? (isZh ? '切暗底' : 'Dark backdrop') : isZh ? '切亮底' : 'Light backdrop'}
                </button>
              </div>
            </div>

            {/* Right 5 cols: Install + 5-Layer Compose + Palette + Specs */}
            <div className="lg:col-span-5 p-6 sm:p-8 lg:overflow-y-auto lg:max-h-[calc(100vh-4rem)] flex flex-col justify-between gap-7">
              <div className="space-y-6">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <h2
                      className="kg-serif text-[26px] font-normal tracking-[0.02em] m-0"
                      style={{ color: 'var(--kg-ink)' }}
                    >
                      {selected.name}
                    </h2>
                    <div
                      className="mt-1 kg-mono"
                      style={{ fontSize: 'var(--kg-text-sm)', color: 'var(--kg-ink-muted)' }}
                    >
                      {selected.id} · v{selected.version}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => setSelected(null)}
                    aria-label={isZh ? '关闭预览' : 'Close preview'}
                    className="bg-transparent border-0 p-1 cursor-pointer text-lg leading-none opacity-60 hover:opacity-100"
                    style={{ color: 'var(--kg-ink)' }}
                  >
                    ✕
                  </button>
                </div>

                {selected.subtitle && (
                  <p className="m-0" style={{ fontSize: 'var(--kg-text-base)', color: 'var(--kg-ink-secondary)' }}>
                    {selected.subtitle}
                  </p>
                )}

                {/* Sharp integrated CLI install command + 5-layer compose command */}
                <div className="space-y-2.5">
                  <div
                    className="flex items-center justify-between gap-3 pl-3.5 pr-1.5 py-1.5 rounded-[2px] border"
                    style={{
                      background: 'var(--kg-surface)',
                      borderColor: 'var(--kg-hairline-strong)',
                    }}
                  >
                    <code
                      className="kg-mono truncate"
                      style={{ fontSize: 'var(--kg-text-sm)', color: 'var(--kg-ink)' }}
                    >
                      <span style={{ color: 'var(--kg-accent)' }}>$</span> {installCommandFor(selected)}
                    </code>
                    <button
                      type="button"
                      onClick={() => copyText(`modal-${selected.id}`, installCommandFor(selected))}
                      className="shrink-0 px-3 py-1.5 rounded-[2px] border-0 cursor-pointer font-medium"
                      style={{
                        fontSize: 'var(--kg-text-sm)',
                        background:
                          copiedKey === `modal-${selected.id}` ? 'var(--kg-success)' : 'var(--kg-accent)',
                        color: '#070A0F',
                        transition: 'background-color 160ms ease',
                      }}
                    >
                      {copiedKey === `modal-${selected.id}`
                        ? isZh
                          ? '已复制'
                          : 'Copied'
                        : isZh
                          ? '复制'
                          : 'Copy'}
                    </button>
                  </div>

                  {/* Authentic Kiseki 5-layer mix command */}
                  <div className="flex items-center justify-between gap-3 px-0.5">
                    <span
                      className="kg-mono text-[11.5px] truncate"
                      style={{ color: 'var(--kg-ink-muted)' }}
                    >
                      {isZh ? '单层混搭：' : 'Layer mix: '}
                      <code style={{ color: 'var(--kg-ink-secondary)' }}>{composeCommandFor(selected)}</code>
                    </span>
                    <button
                      type="button"
                      onClick={() => copyText(`compose-${selected.id}`, composeCommandFor(selected))}
                      className="shrink-0 bg-transparent border-0 p-0 cursor-pointer hover:underline"
                      style={{
                        fontSize: '12px',
                        color:
                          copiedKey === `compose-${selected.id}`
                            ? 'var(--kg-success)'
                            : 'var(--kg-accent)',
                      }}
                    >
                      {copiedKey === `compose-${selected.id}` ? (isZh ? '已复制' : 'Copied') : isZh ? '复制' : 'Copy'}
                    </button>
                  </div>

                  {selected.builtin && (
                    <div className="pt-1">
                      <button
                        type="button"
                        onClick={() => {
                          onTryOfficialTheme(selected.id as ThemeId);
                          setSelected(null);
                        }}
                        className="bg-transparent border-0 p-0 cursor-pointer font-medium hover:underline"
                        style={{ fontSize: 'var(--kg-text-sm)', color: 'var(--kg-accent-rose)' }}
                      >
                        {isZh ? '在首页 WebGL 实时终端试玩 →' : 'Try live in WebGL terminal →'}
                      </button>
                    </div>
                  )}
                </div>

                {/* Seamless 8-swatch designer palette bar */}
                <div>
                  <div className="mb-2 flex items-center justify-between gap-2" style={{ fontSize: 'var(--kg-text-sm)' }}>
                    <span style={{ color: 'var(--kg-ink-muted)' }}>
                      {isZh ? '主题色板' : 'Palette'}
                    </span>
                    <span className="kg-mono" style={{ color: 'var(--kg-ink-secondary)' }}>
                      {copiedKey?.startsWith('hex-')
                        ? isZh
                          ? `已复制 ${copiedKey.slice(4)}`
                          : `Copied ${copiedKey.slice(4)}`
                        : hoveredSwatch
                          ? `${hoveredSwatch.label} · ${hoveredSwatch.hex}`
                          : isZh
                            ? '点击色块复制'
                            : 'Click any color to copy'}
                    </span>
                  </div>
                  <div
                    className="h-8 rounded-[2px] overflow-hidden flex"
                    style={{ boxShadow: 'inset 0 0 0 1px rgba(235,241,255,0.1)' }}
                    onMouseLeave={() => setHoveredSwatch(null)}
                  >
                    {(
                      [
                        ['Base', selected.preview.palette.base],
                        ['Surface', selected.preview.palette.surface0],
                        ['Text', selected.preview.palette.textPrimary],
                        ['Accent', selected.preview.palette.accentPrimary],
                        ['Accent 2', selected.preview.palette.accentSecondary],
                        ['Keyword', selected.preview.palette.keyword],
                        ['String', selected.preview.palette.string],
                        ['Function', selected.preview.palette.function],
                      ] as const
                    ).map(([label, hex]) => (
                      <button
                        key={label}
                        type="button"
                        aria-label={`${label} ${hex}`}
                        title={`${label} ${hex}`}
                        onMouseEnter={() => setHoveredSwatch({ label, hex })}
                        onFocus={() => setHoveredSwatch({ label, hex })}
                        onClick={() => copyText(`hex-${hex}`, hex)}
                        className="kg-palette-swatch"
                        style={{ background: hex }}
                      />
                    ))}
                  </div>
                </div>

                {/* Clean hairline-divided spec rows */}
                <dl className="divide-y m-0" style={{ borderColor: 'var(--kg-hairline)', fontSize: 'var(--kg-text-sm)' }}>
                  {(
                    [
                      [
                        isZh ? '作者' : 'Author',
                        selected.authors.map((a) => a.name).join(', ') || selected.repo.split('/')[0],
                      ],
                      [
                        isZh ? '明暗与对比度' : 'Appearance',
                        `${selected.appearance === 'dark' ? (isZh ? '暗色' : 'Dark') : isZh ? '浅色' : 'Light'} · WCAG ${
                          selected.capabilities.wcagGrade || 'AA'
                        }`,
                      ],
                      [
                        isZh ? '动态 Shader' : 'Live shader',
                        selected.capabilities.hasShader
                          ? selected.capabilities.shaderProfiles.join(' / ') || 'GLSL & HLSL'
                          : isZh
                            ? '无（纯静态）'
                            : 'None (static)',
                      ],
                      [
                        isZh ? '壁纸素材' : 'Wallpaper',
                        selected.capabilities.hasWallpaper ? (isZh ? '包含' : 'Included') : isZh ? '无' : 'None',
                      ],
                      ...(!selected.builtin
                        ? ([
                            [
                              isZh ? '社区热度' : 'Activity',
                              `★ ${selected.metrics.stars} (+${selected.metrics.stars7d}/7d) · ↓ ${selected.metrics.downloads} (+${selected.metrics.downloads30d}/30d)`,
                            ],
                          ] as Array<[string, string]>)
                        : []),
                      [isZh ? '许可证' : 'License', selected.license || 'MIT'],
                    ] as Array<[string, string]>
                  ).map(([label, value]) => (
                    <div
                      key={label}
                      className="py-2.5 flex items-center justify-between gap-4"
                      style={{ borderColor: 'var(--kg-hairline)' }}
                    >
                      <dt style={{ color: 'var(--kg-ink-muted)' }}>{label}</dt>
                      <dd className="m-0 font-medium text-right kg-tabular" style={{ color: 'var(--kg-ink)' }}>
                        {value}
                      </dd>
                    </div>
                  ))}
                </dl>

                {/* Lazy-loaded release notes */}
                {!selected.builtin && updatesMap?.themes[selected.id]?.releaseNotes && (
                  <div
                    className="pt-3 border-t space-y-1.5"
                    style={{ borderColor: 'var(--kg-hairline)', fontSize: 'var(--kg-text-sm)' }}
                  >
                    <div className="font-medium" style={{ color: 'var(--kg-ink)' }}>
                      {isZh ? '更新说明' : 'Release notes'}
                    </div>
                    <p
                      className="m-0 whitespace-pre-wrap leading-relaxed"
                      style={{ color: 'var(--kg-ink-secondary)' }}
                    >
                      {updatesMap.themes[selected.id]!.releaseNotes}
                    </p>
                  </div>
                )}
              </div>

              {/* Footer links */}
              <div
                className="pt-4 border-t flex flex-wrap items-center justify-between gap-3"
                style={{
                  borderColor: 'var(--kg-hairline)',
                  fontSize: 'var(--kg-text-sm)',
                }}
              >
                <a
                  href={`https://github.com/${selected.repo}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="font-medium hover:underline no-underline"
                  style={{ color: 'var(--kg-accent)' }}
                >
                  GitHub ({selected.repo}) ↗
                </a>
                {!selected.builtin && (
                  <a
                    href={`${GALLERY_REGISTRY_REPO}/issues/new?template=takedown.yml&title=${encodeURIComponent(
                      `[Takedown] ${selected.id}`,
                    )}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="hover:underline no-underline"
                    style={{ color: 'var(--kg-ink-muted)' }}
                  >
                    {isZh ? '版权或下架反馈' : 'Report / Takedown'}
                  </a>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </main>
  );
};
