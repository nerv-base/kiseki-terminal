import type { FC } from 'react';
import { I18N } from '../data/worlds';
import type { Lang } from '../types/theme';

interface PromisesSectionProps {
  lang: Lang;
}

export const PromisesSection: FC<PromisesSectionProps> = ({ lang }) => {
  const t = I18N[lang];

  return (
    <section
      id="system"
      className="relative z-10 py-24 sm:py-32 border-t overflow-hidden"
      style={{ borderColor: 'color-mix(in srgb, var(--k-border) 22%, transparent)' }}
    >
      <div
        className="pointer-events-none absolute -bottom-24 right-1/4 w-[38rem] h-[19rem] rounded-full blur-3xl opacity-[0.06]"
        style={{ background: 'radial-gradient(circle, var(--k-accent-secondary), transparent 70%)' }}
      />

      <div className="k-reveal max-w-7xl mx-auto px-6 sm:px-10 space-y-14">
        <div
          className="flex items-end justify-between gap-6 border-b pb-5"
          style={{ borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)' }}
        >
          <div className="space-y-2 max-w-2xl">
            <div className="font-k-hud text-xs tracking-[0.26em] uppercase" style={{ color: 'var(--k-accent-secondary)' }}>
              02 // WHY KISEKI
            </div>
            <h2 className="k-act-heading text-2xl sm:text-4xl m-0" style={{ color: 'var(--k-text-primary)' }}>
              {t.act2_title}
            </h2>
          </div>
          <div className="hidden sm:flex items-center shrink-0 select-none">
            <span
              className="k-vertical-jp font-k-serif text-xs px-1.5 py-2 border opacity-75"
              style={{
                borderColor: 'color-mix(in srgb, var(--k-border) 50%, transparent)',
                color: 'var(--k-accent-primary)',
              }}
            >
              第弐幕・設計
            </span>
          </div>
        </div>

        <div
          className="grid grid-cols-1 md:grid-cols-3 divide-y md:divide-y-0 md:divide-x"
          style={{ borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)' }}
        >
          <div className="k-col-accent py-8 md:py-0 md:pr-10 pt-8 space-y-4" style={{ borderColor: 'inherit' }}>
            <div className="k-col-num text-3xl opacity-75" style={{ color: 'var(--k-accent-primary)' }}>
              01
            </div>
            <h3 className="text-lg font-bold m-0" style={{ color: 'var(--k-text-primary)' }}>
              {t.col1_t}
            </h3>
            <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
              {lang === 'zh' ? (
                <>
                  只加 3 行引用，不改 <code className="font-k-mono text-xs">ls</code>/<code className="font-k-mono text-xs">cat</code> 与快捷键，随时{' '}
                  <code className="font-k-mono text-xs" style={{ color: 'var(--k-accent-primary)' }}>
                    kiseki rollback
                  </code>{' '}
                  一键还原。
                </>
              ) : (
                <>
                  Appends only a 3-line include. Zero <code className="font-k-mono text-xs">ls</code>/
                  <code className="font-k-mono text-xs">cat</code> hijacks. Instant undo via{' '}
                  <code className="font-k-mono text-xs" style={{ color: 'var(--k-accent-primary)' }}>
                    kiseki rollback
                  </code>
                  .
                </>
              )}
            </p>
            <pre
              className="m-0 pt-3 border-t font-k-mono text-xs leading-relaxed overflow-x-auto"
              style={{
                borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)',
                color: 'var(--k-success)',
              }}
            >
              + source &quot;~/.local/share/kiseki/init.zsh&quot;
            </pre>
          </div>

          <div className="k-col-accent py-8 md:py-0 md:px-10 pt-8 space-y-4" style={{ borderColor: 'inherit' }}>
            <div className="k-col-num text-3xl opacity-75" style={{ color: 'var(--k-accent-secondary)' }}>
              02
            </div>
            <h3 className="text-lg font-bold m-0" style={{ color: 'var(--k-text-primary)' }}>
              {t.col2_t}
            </h3>
            <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
              {lang === 'zh' ? (
                <>
                  壁纸、光影、配色、字体 5 层自由组合，内置{' '}
                  <code className="font-k-mono text-xs" style={{ color: 'var(--k-accent-secondary)' }}>
                    naga
                  </code>{' '}
                  静态校验零脚本风险。
                </>
              ) : (
                <>
                  Combine wallpaper, shader, palette &amp; font across 5 layers—validated by{' '}
                  <code className="font-k-mono text-xs" style={{ color: 'var(--k-accent-secondary)' }}>
                    naga
                  </code>{' '}
                  with zero scripts.
                </>
              )}
            </p>
            <div
              className="pt-3 border-t font-k-mono text-xs"
              style={{
                borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)',
                color: 'var(--k-accent-primary)',
              }}
            >
              kiseki compose --palette kyoto-sakura --atmosphere starlight-8bit
            </div>
          </div>

          <div className="k-col-accent py-8 md:py-0 md:pl-10 pt-8 space-y-4" style={{ borderColor: 'inherit' }}>
            <div className="k-col-num text-3xl opacity-75" style={{ color: 'var(--k-accent-tertiary)' }}>
              03
            </div>
            <h3 className="text-lg font-bold m-0" style={{ color: 'var(--k-text-primary)' }}>
              {t.col3_t}
            </h3>
            <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
              {lang === 'zh' ? (
                <>
                  <span className="font-k-mono">8.18 MiB</span> 单文件内置 4 套主题，无需 Python；冷启动{' '}
                  <span className="font-k-mono">4.3 ms</span>，多窗口秒同步。
                </>
              ) : (
                <>
                  <span className="font-k-mono">8.18 MiB</span> binary bundles 4 themes with no Python;{' '}
                  <span className="font-k-mono">4.3 ms</span> startup and instant multi-window sync.
                </>
              )}
            </p>
            <div
              className="pt-3 border-t flex flex-wrap items-center gap-x-6 gap-y-2 font-k-mono"
              style={{ borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)' }}
            >
              <div>
                <span className="text-base font-bold k-tabular" style={{ color: 'var(--k-accent-primary)' }}>
                  4.3 ms
                </span>
                <span className="text-[0.6875rem] ml-1" style={{ color: 'var(--k-text-muted)' }}>
                  {t.m_start}
                </span>
              </div>
              <div>
                <span className="text-base font-bold k-tabular" style={{ color: 'var(--k-accent-secondary)' }}>
                  54.7 µs
                </span>
                <span className="text-[0.6875rem] ml-1" style={{ color: 'var(--k-text-muted)' }}>
                  {t.m_hook}
                </span>
              </div>
              <div>
                <span className="text-base font-bold k-tabular" style={{ color: 'var(--k-text-primary)' }}>
                  8.18 MB
                </span>
                <span className="text-[0.6875rem] ml-1" style={{ color: 'var(--k-text-muted)' }}>
                  {t.m_bin}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
};
