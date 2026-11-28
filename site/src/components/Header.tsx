import type { FC } from 'react';
import { I18N } from '../data/worlds';
import type { GalleryCanvasMode } from './GalleryHub';
import type { Lang, ViewMode } from '../types/theme';

interface HeaderProps {
  view: ViewMode;
  scrolled: boolean;
  lang: Lang;
  galleryMode?: GalleryCanvasMode;
  onNavigate: (target: 'home' | 'terminal' | 'gallery' | 'download' | 'docs') => void;
  onToggleLang: () => void;
}

export const Header: FC<HeaderProps> = ({
  view,
  scrolled,
  lang,
  galleryMode = 'dark',
  onNavigate,
  onToggleLang,
}) => {
  const t = I18N[lang];
  const isHomeActive = view === 'landing';
  const isGalleryActive = view === 'gallery';
  const isDocsActive = view === 'docs';

  const navPrimary = isGalleryActive ? '#EFF3FA' : 'var(--k-text-primary)';
  const navSecondary = isGalleryActive ? '#94A0B6' : 'var(--k-text-secondary)';
  const navAccent = isGalleryActive ? '#78D6EE' : 'var(--k-accent-primary)';
  const navDivider = isGalleryActive ? 'rgba(235,241,255,0.16)' : 'var(--k-text-muted)';

  return (
    <header
      id="k-header"
      className={`fixed top-0 left-0 right-0 z-40 ${
        isGalleryActive
          ? 'kg-header-bar h-[60px]'
          : scrolled || view !== 'landing'
            ? 'h-16 is-scrolled'
            : 'h-16'
      }`}
      data-kg-mode={isGalleryActive ? galleryMode : undefined}
      style={
        isGalleryActive
          ? {
              background: 'rgba(7, 10, 15, 0.86)',
              borderBottom: '1px solid rgba(235, 241, 255, 0.08)',
            }
          : undefined
      }
    >
      <div
        className={
          isGalleryActive
            ? 'kg-container h-full flex items-center justify-between gap-3'
            : 'max-w-[120rem] mx-auto h-full px-4 sm:px-10 lg:px-16 flex items-center justify-between gap-3'
        }
      >
        <button
          id="k-logo-home-btn"
          type="button"
          onClick={() => onNavigate('home')}
          className="bg-transparent border-0 p-0 cursor-pointer flex items-center gap-2.5"
          style={{ color: navPrimary }}
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 32 32"
            shapeRendering="crispEdges"
            className="w-[1.375rem] h-[1.375rem] shrink-0"
            aria-hidden="true"
          >
            <rect x="4" y="4" width="26" height="26" fill="#FF82B8" fillOpacity="0.34" />
            <rect x="2" y="2" width="26" height="26" fill="#66F0FF" fillOpacity="0.28" />
            <rect x="3" y="3" width="26" height="26" fill="#0A0F25" />
            <rect x="4" y="2" width="24" height="1" fill="#7AA2F7" />
            <rect x="4" y="29" width="24" height="1" fill="#7AA2F7" />
            <rect x="2" y="4" width="1" height="24" fill="#7AA2F7" />
            <rect x="29" y="4" width="1" height="24" fill="#7AA2F7" />
            <rect x="3" y="3" width="1" height="1" fill="#66F0FF" />
            <rect x="28" y="3" width="1" height="1" fill="#FF82B8" />
            <rect x="3" y="28" width="1" height="1" fill="#66F0FF" />
            <rect x="28" y="28" width="1" height="1" fill="#FF82B8" />
            <rect x="4" y="4" width="24" height="4" fill="#172046" />
            <rect x="5" y="5" width="2" height="2" fill="#FF6B8B" />
            <rect x="8" y="5" width="2" height="2" fill="#FFD580" />
            <rect x="11" y="5" width="2" height="2" fill="#72F1B8" />
            <rect x="4" y="8" width="24" height="1" fill="#2A376E" />
            <rect x="7" y="13" width="2" height="2" fill="#66F0FF" />
            <rect x="9" y="15" width="2" height="2" fill="#66F0FF" />
            <rect x="11" y="17" width="2" height="2" fill="#99F7FF" />
            <rect x="9" y="19" width="2" height="2" fill="#66F0FF" />
            <rect x="7" y="21" width="2" height="2" fill="#66F0FF" />
            <rect x="14" y="21" width="6" height="2" fill="#FF82B8" />
            <rect x="22" y="11" width="1" height="5" fill="#FFE5A3" />
            <rect x="20" y="13" width="5" height="1" fill="#FFE5A3" />
            <rect x="21" y="12" width="3" height="3" fill="#FFF8DC" />
            <rect x="22" y="13" width="1" height="1" fill="#FFFFFF" />
            <rect x="17" y="11" width="1" height="1" fill="#FF82B8" />
            <rect x="25" y="17" width="1" height="1" fill="#66F0FF" />
            <rect x="23" y="22" width="2" height="2" fill="#B48EFE" />
            <rect x="5" y="26" width="22" height="1" fill="#1D2754" />
          </svg>
          <span
            className={isGalleryActive ? 'text-[13.5px] font-semibold tracking-[0.14em] uppercase whitespace-nowrap' : 'font-k-display text-xs tracking-[0.28em] uppercase whitespace-nowrap'}
            style={isGalleryActive ? undefined : { fontWeight: 'var(--k-display-weight)' }}
          >
            KISEKI
          </span>
          {isGalleryActive ? (
            <span
              className="hidden sm:inline kg-mono text-[12px]"
              style={{ color: navSecondary }}
            >
              v2
            </span>
          ) : (
            <span
              className="font-k-mono text-[0.6875rem] px-1.5 py-0.5 border opacity-85 whitespace-nowrap"
              style={{
                borderColor: 'color-mix(in srgb, var(--k-border) 60%, transparent)',
                color: navAccent,
              }}
            >
              v2
            </span>
          )}
        </button>

        <nav
          className={`flex items-center gap-3.5 sm:gap-7 whitespace-nowrap ${
            isGalleryActive ? 'text-[13px] sm:text-[13.5px] font-medium' : 'font-k-mono text-xs tracking-[0.12em]'
          }`}
          style={{ color: navSecondary }}
        >
          <button
            type="button"
            onClick={() => onNavigate('home')}
            className="k-nav-btn whitespace-nowrap bg-transparent border-0 p-0 pb-1 cursor-pointer"
            style={{
              color: isHomeActive ? navAccent : navSecondary,
              borderBottom: isHomeActive ? `1.5px solid ${navAccent}` : '1.5px solid transparent',
            }}
          >
            {t.nav_home}
          </button>
          <button
            type="button"
            onClick={() => onNavigate('terminal')}
            className="k-nav-btn hidden sm:inline whitespace-nowrap bg-transparent border-0 p-0 pb-1 cursor-pointer"
            style={{ color: navSecondary, borderBottom: '1.5px solid transparent' }}
          >
            {t.nav_try}
          </button>
          <button
            type="button"
            onClick={() => onNavigate('gallery')}
            className="k-nav-btn whitespace-nowrap bg-transparent border-0 p-0 pb-1 cursor-pointer"
            style={{
              color: isGalleryActive ? navAccent : navSecondary,
              borderBottom: isGalleryActive ? `1.5px solid ${navAccent}` : '1.5px solid transparent',
            }}
          >
            {t.nav_gallery}
          </button>
          <button
            type="button"
            onClick={() => onNavigate('download')}
            className="k-nav-btn whitespace-nowrap bg-transparent border-0 p-0 pb-1 cursor-pointer"
            style={{ color: navSecondary, borderBottom: '1.5px solid transparent' }}
          >
            {t.nav_dl}
          </button>
          <button
            type="button"
            onClick={() => onNavigate('docs')}
            className="k-nav-btn whitespace-nowrap bg-transparent border-0 p-0 pb-1 cursor-pointer"
            style={{
              color: isDocsActive ? navAccent : navSecondary,
              borderBottom: isDocsActive ? `1.5px solid ${navAccent}` : '1.5px solid transparent',
            }}
          >
            {t.nav_docs}
          </button>
          <span className="hidden sm:inline-block h-3.5 w-px opacity-40" style={{ background: navDivider }} />
          <button
            id="k-lang-btn"
            type="button"
            onClick={onToggleLang}
            className={`cursor-pointer whitespace-nowrap bg-transparent border-0 p-0 ${
              isGalleryActive ? 'text-[0.8125rem] font-medium' : 'font-k-mono text-xs tracking-widest'
            } opacity-90 hover:opacity-100`}
            style={{ color: isGalleryActive ? navSecondary : navAccent }}
          >
            <span className="hidden sm:inline">中 / EN</span>
            <span className="sm:hidden">{lang === 'zh' ? 'EN' : '中'}</span>
          </button>
          <a
            href="https://github.com/nerv-base/kiseki-terminal"
            target="_blank"
            rel="noopener noreferrer"
            aria-label="GitHub"
            title="nerv-base/kiseki-terminal"
            className="hidden sm:inline-flex items-center opacity-80 hover:opacity-100 transition-opacity"
            style={{ color: navPrimary }}
          >
            <svg viewBox="0 0 16 16" className="w-4 h-4" fill="currentColor" aria-hidden="true">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
            </svg>
          </a>
        </nav>
      </div>
    </header>
  );
};
