import { useState, type FC } from 'react';
import { KISEKI_BUNDLE } from '../data/bundle';
import { I18N, INSTALL_SNIPPETS_BY_REGION, THEME_ORDER, WORLD_META, type DetectedOS } from '../data/worlds';
import type { DocChapterId, InstallRegion, InstallTab, Lang, ThemeId } from '../types/theme';

interface InstallSectionProps {
  lang: Lang;
  atmosphereId: ThemeId;
  detectedOS: DetectedOS;
  installTab: InstallTab;
  installRegion: InstallRegion;
  onSelectInstallTab: (tab: InstallTab) => void;
  onSelectInstallRegion: (region: InstallRegion) => void;
  onOpenDoc: (docId: DocChapterId) => void;
}

const OS_BADGE_LABELS: Record<DetectedOS, string> = {
  macos: 'MACOS DETECTED',
  windows: 'WINDOWS DETECTED',
  linux: 'LINUX DETECTED',
};

const INSTALL_TABS: Array<{ id: InstallTab; label: string }> = [
  { id: 'brew', label: 'Homebrew' },
  { id: 'curl', label: 'curl (macOS / Linux)' },
  { id: 'ps1', label: 'PowerShell (ps1)' },
];

export const InstallSection: FC<InstallSectionProps> = ({
  lang,
  atmosphereId,
  detectedOS,
  installTab,
  installRegion,
  onSelectInstallTab,
  onSelectInstallRegion,
  onOpenDoc,
}) => {
  const [copied, setCopied] = useState(false);
  const t = I18N[lang];
  const meta = WORLD_META[atmosphereId];
  const snippet = INSTALL_SNIPPETS_BY_REGION[installRegion][installTab];

  const handleCopy = () => {
    const done = () => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1300);
    };
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(snippet).then(done).catch(done);
    } else {
      done();
    }
  };

  return (
    <section
      id="download"
      className="relative z-10 py-24 sm:py-36 border-t overflow-hidden"
      style={{ borderColor: 'color-mix(in srgb, var(--k-border) 22%, transparent)' }}
    >
      {/* Pre-mounted 4-image Epilogue Artwork Wash (instant opacity crossfade) */}
      <div id="k-epilogue-backdrop" className="pointer-events-none absolute inset-0 overflow-hidden">
        {THEME_ORDER.map((id) => (
          <img
            key={id}
            src={KISEKI_BUNDLE.themes[id].wallpaperUrl}
            alt=""
            className="absolute inset-0 w-full h-full object-cover object-right-bottom transition-opacity duration-500"
            style={{
              maskImage: 'radial-gradient(ellipse 75% 75% at 68% 50%, #000 10%, transparent 90%)',
              WebkitMaskImage: 'radial-gradient(ellipse 75% 75% at 68% 50%, #000 10%, transparent 90%)',
              opacity: id === atmosphereId ? 0.14 : 0,
            }}
          />
        ))}
        <div
          className="absolute inset-0 z-10"
          style={{
            background: 'linear-gradient(180deg, var(--k-crust) 0%, transparent 25%, transparent 75%, var(--k-crust) 100%)',
          }}
        />
      </div>

      <div className="k-reveal relative z-10 max-w-5xl mx-auto px-6 sm:px-10 space-y-12">
        <div
          className="flex flex-col sm:flex-row sm:items-end justify-between gap-4 border-b pb-5"
          style={{ borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)' }}
        >
          <div className="space-y-2">
            <div className="font-k-hud text-xs tracking-[0.26em] uppercase" style={{ color: 'var(--k-accent-primary)' }}>
              03 // INSTALL
            </div>
            <h2 className="k-act-heading text-2xl sm:text-4xl m-0" style={{ color: 'var(--k-text-primary)' }}>
              {t.act3_title}
            </h2>
          </div>
          <div className="flex flex-wrap items-center gap-3 font-k-hud text-xs">
            <div
              id="k-region-toggle"
              className="inline-flex border p-0.5 font-k-mono text-[0.6875rem]"
              style={{
                borderColor: 'color-mix(in srgb, var(--k-border) 48%, transparent)',
                background: 'color-mix(in srgb, var(--k-crust) 70%, transparent)',
                borderRadius: 'var(--k-radius)',
              }}
            >
              {(['cn', 'global'] as const).map((region) => {
                const active = installRegion === region;
                return (
                  <button
                    key={region}
                    type="button"
                    data-region={region}
                    onClick={() => onSelectInstallRegion(region)}
                    className="bg-transparent border-0 px-2.5 py-1 cursor-pointer transition-colors"
                    style={{
                      background: active ? 'var(--k-surface1)' : 'transparent',
                      color: active ? 'var(--k-accent-primary)' : 'var(--k-text-muted)',
                      fontWeight: active ? 700 : 400,
                    }}
                  >
                    {region === 'cn' ? t.region_cn : t.region_global}
                  </button>
                );
              })}
            </div>
            <span
              id="k-detected-os-badge"
              className="font-k-hud px-2.5 py-1 border"
              style={{
                borderColor: 'var(--k-accent-secondary)',
                color: 'var(--k-accent-secondary)',
                background: 'color-mix(in srgb, var(--k-crust) 70%, transparent)',
              }}
            >
              {OS_BADGE_LABELS[detectedOS]}
            </span>
          </div>
        </div>

        <div
          className="k-cinema-corners relative max-w-4xl mx-auto border backdrop-blur-md p-6 sm:p-8 space-y-5"
          style={{
            background: 'color-mix(in srgb, var(--k-crust) 80%, transparent)',
            borderColor: 'color-mix(in srgb, var(--k-border) 38%, transparent)',
            boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.06)',
          }}
        >
          <div className="font-k-mono text-xs uppercase tracking-wider" style={{ color: 'var(--k-text-muted)' }}>
            {t.dl_cli_hdr}
          </div>
          <div
            className="flex flex-wrap items-center gap-6 border-b pb-2.5 font-k-mono text-xs"
            style={{ borderColor: 'color-mix(in srgb, var(--k-border) 26%, transparent)' }}
            id="k-inst-tabs"
          >
            {INSTALL_TABS.map((tab) => {
              const active = installTab === tab.id;
              return (
                <button
                  key={tab.id}
                  type="button"
                  data-inst={tab.id}
                  onClick={() => onSelectInstallTab(tab.id)}
                  className="k-inst-tab bg-transparent border-0 p-0 pb-1 cursor-pointer"
                  style={{
                    color: active ? 'var(--k-accent-primary)' : 'var(--k-text-muted)',
                    borderBottom: active ? '2px solid var(--k-accent-primary)' : '2px solid transparent',
                  }}
                >
                  {tab.label}
                </button>
              );
            })}
          </div>

          <div
            className="p-4 sm:p-5 border flex items-start justify-between gap-3 font-k-mono text-xs leading-relaxed"
            style={{
              background: 'color-mix(in srgb, var(--k-base) 78%, transparent)',
              borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)',
              borderRadius: 'var(--k-radius)',
            }}
          >
            <pre id="k-inst-code" className="m-0 overflow-x-auto" style={{ color: 'var(--k-text-primary)' }}>
              {snippet}
            </pre>
            <button
              id="k-inst-copy-btn"
              type="button"
              onClick={handleCopy}
              className="shrink-0 bg-transparent border-0 p-0 cursor-pointer font-k-mono text-xs font-bold tracking-wider uppercase"
              style={{ color: 'var(--k-accent-primary)' }}
            >
              {copied ? t.copied : t.copy}
            </button>
          </div>

          <div className="flex flex-wrap items-center justify-between gap-3 pt-1 font-k-mono text-xs">
            <span style={{ color: 'var(--k-text-muted)' }}>
              {t.dl_init_tip_pre}
              <code style={{ color: 'var(--k-accent-primary)' }}>kiseki init</code>
              {t.dl_init_tip_post}
            </span>
            <button
              type="button"
              onClick={() => onOpenDoc('quickstart')}
              className="bg-transparent border-0 p-0 cursor-pointer underline"
              style={{ color: 'var(--k-accent-secondary)' }}
            >
              {t.dl_docs_link}
            </button>
          </div>
        </div>

        <footer className="pt-10 space-y-4 font-k-mono text-[0.6875rem] tracking-[0.16em] text-center" style={{ color: 'var(--k-text-muted)' }}>
          <div className="font-k-hud flex flex-wrap items-center justify-center gap-x-6 gap-y-2 opacity-75 uppercase">
            <span>ENGINE: RUST SINGLE BINARY</span>
            <span>·</span>
            <span>SHADERS: NAGA VALIDATED</span>
            <span>·</span>
            <span>LICENSE: MIT</span>
          </div>
          <div
            className="pt-3 border-t flex flex-col sm:flex-row items-center justify-between gap-3 text-xs tracking-normal"
            style={{ borderColor: 'color-mix(in srgb, var(--k-border) 24%, transparent)' }}
          >
            <div className="flex flex-wrap items-center justify-center gap-2">
              <span>KISEKI TERMINAL v2 ·</span>
              <a
                href="https://github.com/nerv-base/kiseki-terminal"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 no-underline hover:underline"
                style={{ color: 'var(--k-text-primary)' }}
              >
                <svg viewBox="0 0 16 16" className="w-[0.875rem] h-[0.875rem]" fill="currentColor" aria-hidden="true">
                  <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
                </svg>
                <span>nerv-base/kiseki-terminal</span>
              </a>
              <span>· kisekiterm.dev</span>
            </div>
            <div id="k-ed-egg" style={{ color: 'var(--k-accent-secondary)' }}>
              {meta.egg}
            </div>
          </div>
        </footer>
      </div>
    </section>
  );
};
