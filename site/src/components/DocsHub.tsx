import { useMemo, useState, type FC } from 'react';
import { CLI_COMMANDS, COMPOSE_LAYERS, DOC_CHAPTERS_META } from '../data/docs';
import { I18N } from '../data/worlds';
import type { DocChapterId, Lang } from '../types/theme';

interface DocsHubProps {
  lang: Lang;
  docChapter: DocChapterId;
  docSearch: string;
  onSelectChapter: (id: DocChapterId) => void;
  onSearchChange: (q: string) => void;
  onNavigate: (target: 'home' | 'download') => void;
}

const CodeBlock: FC<{ code: string; lang: Lang }> = ({ code, lang }) => {
  const [copied, setCopied] = useState(false);
  const t = I18N[lang];

  const handleCopy = () => {
    const done = () => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1300);
    };
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(code).then(done).catch(done);
    } else {
      done();
    }
  };

  return (
    <div
      className="relative p-4 border font-k-mono text-xs leading-relaxed"
      style={{
        background: 'color-mix(in srgb, var(--k-mantle) 80%, transparent)',
        borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)',
        borderRadius: 'var(--k-radius)',
      }}
    >
      <button
        type="button"
        onClick={handleCopy}
        className="absolute top-2.5 right-2.5 bg-transparent border-0 p-0 cursor-pointer font-k-mono text-[10px] font-bold"
        style={{ color: 'var(--k-accent-primary)' }}
      >
        {copied ? t.copied : t.copy}
      </button>
      <pre className="m-0 overflow-x-auto pr-12" style={{ color: 'var(--k-text-primary)' }}>
        {code}
      </pre>
    </div>
  );
};

export const DocsHub: FC<DocsHubProps> = ({
  lang,
  docChapter,
  docSearch,
  onSelectChapter,
  onSearchChange,
  onNavigate,
}) => {
  const t = I18N[lang];
  const q = docSearch.trim().toLowerCase();

  const filteredChapters = useMemo(
    () =>
      DOC_CHAPTERS_META.filter((ch) => {
        if (!q) return true;
        return (
          ch.titleZh.toLowerCase().includes(q) ||
          ch.titleEn.toLowerCase().includes(q) ||
          ch.keywords.toLowerCase().includes(q)
        );
      }),
    [q],
  );

  const activeCh =
    filteredChapters.find((c) => c.id === docChapter) ?? filteredChapters[0] ?? DOC_CHAPTERS_META[0]!;

  const activeIdx = DOC_CHAPTERS_META.findIndex((c) => c.id === activeCh.id);
  const prevCh = activeIdx > 0 ? DOC_CHAPTERS_META[activeIdx - 1] : null;
  const nextCh = activeIdx < DOC_CHAPTERS_META.length - 1 ? DOC_CHAPTERS_META[activeIdx + 1] : null;

  const selectWithScroll = (id: DocChapterId) => {
    onSelectChapter(id);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  return (
    <div id="k-view-docs" className="relative z-10 min-h-screen pt-24 pb-24">
      <div className="pointer-events-none fixed inset-0 k-blueprint-wash opacity-70" />
      <div className="relative z-10 max-w-6xl mx-auto px-5 sm:px-10">
        <div className="flex flex-col lg:flex-row gap-10 items-start">
          <aside className="w-full lg:w-64 shrink-0 space-y-5 lg:sticky lg:top-24">
            <div className="space-y-1">
              <div className="font-k-mono text-[10px] tracking-[0.24em] uppercase" style={{ color: 'var(--k-accent-primary)' }}>
                DOCUMENTATION
              </div>
              <div className="font-k-serif text-xl" style={{ color: 'var(--k-text-primary)' }}>
                {t.docs_title}
              </div>
            </div>

            <div
              className="border px-3 py-2 flex items-center gap-2 font-k-mono text-xs"
              style={{
                background: 'color-mix(in srgb, var(--k-mantle) 80%, transparent)',
                borderColor: 'color-mix(in srgb, var(--k-border) 45%, transparent)',
                borderRadius: 'var(--k-radius)',
              }}
            >
              <span style={{ color: 'var(--k-accent-primary)' }}>/</span>
              <input
                id="k-docs-search"
                type="text"
                value={docSearch}
                onChange={(e) => onSearchChange(e.target.value)}
                autoComplete="off"
                spellCheck={false}
                placeholder={t.docs_search_placeholder}
                className="w-full bg-transparent border-0 outline-none font-k-mono text-xs"
                style={{ color: 'var(--k-text-primary)' }}
              />
            </div>

            <nav id="k-docs-sidebar" className="space-y-1 font-k-mono text-xs">
              {filteredChapters.map((ch) => {
                const active = ch.id === activeCh.id;
                return (
                  <button
                    key={ch.id}
                    type="button"
                    onClick={() => selectWithScroll(ch.id)}
                    className="w-full text-left px-3 py-2 cursor-pointer border-l-2 bg-transparent transition-colors flex items-center gap-2.5"
                    style={{
                      borderColor: active ? 'var(--k-accent-primary)' : 'color-mix(in srgb, var(--k-border) 30%, transparent)',
                      color: active ? 'var(--k-accent-primary)' : 'var(--k-text-secondary)',
                      fontWeight: active ? 700 : 400,
                    }}
                  >
                    <span className="opacity-65">{ch.num}</span>
                    <span>{lang === 'zh' ? ch.titleZh : ch.titleEn}</span>
                  </button>
                );
              })}
            </nav>

            <div
              className="pt-4 border-t flex items-center justify-between font-k-mono text-[11px]"
              style={{ borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)' }}
            >
              <button
                type="button"
                onClick={() => onNavigate('home')}
                className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                style={{ color: 'var(--k-text-muted)' }}
              >
                {t.back_home}
              </button>
              <button
                type="button"
                onClick={() => onNavigate('download')}
                className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                style={{ color: 'var(--k-accent-secondary)' }}
              >
                {t.go_dl}
              </button>
            </div>
          </aside>

          <article className="flex-1 min-w-0 w-full space-y-8">
            <div id="k-docs-article" className="k-docs-prose space-y-6">
              {activeCh.id === 'quickstart' && (
                <div className="space-y-6">
                  <div className="space-y-2 border-b pb-4" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)' }}>
                    <div className="font-k-mono text-[10px] tracking-[0.2em]" style={{ color: 'var(--k-accent-primary)' }}>
                      CHAPTER 01 // QUICKSTART
                    </div>
                    <h1 className="k-act-heading text-3xl m-0">{lang === 'zh' ? '快速开始' : 'Quickstart'}</h1>
                    <p className="text-sm m-0" style={{ color: 'var(--k-text-secondary)' }}>
                      {lang === 'zh'
                        ? 'Kiseki v2 是零运行时依赖的 Rust 单二进制引擎，内置全部 4 套官方二次元主题与 Shader。'
                        : 'Kiseki v2 ships as a single-binary Rust engine with zero Python dependencies and all 4 anime themes bundled offline.'}
                    </p>
                  </div>

                  <div className="space-y-3">
                    <h3 className="text-base font-bold m-0">{lang === 'zh' ? '1. 命令行安装' : '1. Install Kiseki'}</h3>
                    <CodeBlock
                      lang={lang}
                      code={`# macOS / Linux (Homebrew)\nbrew install nerv-base/tap/kiseki\n\n# macOS / Linux (curl)\ncurl -fsSL https://kisekiterm.dev/install.sh | sh\n\n# Windows (PowerShell)\nirm https://kisekiterm.dev/install.ps1 | iex`}
                    />
                  </div>

                  <div className="space-y-3">
                    <h3 className="text-base font-bold m-0">{lang === 'zh' ? '2. 运行交互向导' : '2. Initialize Non-Invasively'}</h3>
                    <CodeBlock lang={lang} code="kiseki init" />
                    <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
                      {lang === 'zh' ? (
                        <>
                          向导会自动探测 Ghostty / Windows Terminal / Zsh / PowerShell 环境，只追加外部{' '}
                          <code className="font-k-mono">include</code> 挂载；如果你使用 Nix 或 Home Manager，可用{' '}
                          <code className="font-k-mono">kiseki init --render-only</code> 仅生成产物。
                        </>
                      ) : (
                        <>
                          Auto-detects Ghostty, Windows Terminal, Zsh, and PowerShell. Uses clean external includes or comment-preserving JSONC patches. Pass{' '}
                          <code className="font-k-mono">--render-only</code> for Nix / Home Manager.
                        </>
                      )}
                    </p>
                  </div>

                  <div className="space-y-3">
                    <h3 className="text-base font-bold m-0">{lang === 'zh' ? '3. 推荐配套工具（可选）' : '3. Optional Companions'}</h3>
                    <CodeBlock lang={lang} code="brew install oh-my-posh fastfetch zsh-syntax-highlighting" />
                    <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
                      {lang === 'zh' ? (
                        <>
                          安装后即可解锁主题专属 Prompt 段（如 <code className="font-k-mono">[弐号機]</code> /{' '}
                          <code className="font-k-mono">❀ 夜桜</code>）、像素系统信息卡与 24 项命令实时语法高亮。
                        </>
                      ) : (
                        <>Unlocks themed Oh My Posh prompts, TrueColor pixel Fastfetch cards, and 24-token live Zsh command highlighting.</>
                      )}
                    </p>
                  </div>
                </div>
              )}

              {activeCh.id === 'cli' && (
                <div className="space-y-6">
                  <div className="space-y-2 border-b pb-4" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)' }}>
                    <div className="font-k-mono text-[10px] tracking-[0.2em]" style={{ color: 'var(--k-accent-primary)' }}>
                      CHAPTER 02 // CLI REFERENCE
                    </div>
                    <h1 className="k-act-heading text-3xl m-0">{lang === 'zh' ? '全部 15 个核心命令' : 'All 15 Core Commands'}</h1>
                  </div>
                  <div className="divide-y font-k-mono text-xs" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 25%, transparent)' }}>
                    {CLI_COMMANDS.filter(
                      ([c, zh, en]) => !q || c.toLowerCase().includes(q) || zh.toLowerCase().includes(q) || en.toLowerCase().includes(q),
                    ).map(([c, zh, en]) => (
                      <div key={c} className="py-3 flex flex-col sm:flex-row sm:items-baseline justify-between gap-2">
                        <code className="font-bold shrink-0" style={{ color: 'var(--k-accent-primary)' }}>
                          {c}
                        </code>
                        <span className="font-sans text-xs sm:text-right" style={{ color: 'var(--k-text-secondary)' }}>
                          {lang === 'zh' ? zh : en}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {activeCh.id === 'compose' && (
                <div className="space-y-6">
                  <div className="space-y-2 border-b pb-4" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)' }}>
                    <div className="font-k-mono text-[10px] tracking-[0.2em]" style={{ color: 'var(--k-accent-primary)' }}>
                      CHAPTER 03 // THEMES & COMPOSE
                    </div>
                    <h1 className="k-act-heading text-3xl m-0">{lang === 'zh' ? '主题架构与 5 层混搭' : '5-Layer Token Composition'}</h1>
                  </div>
                  <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
                    {lang === 'zh'
                      ? 'Kiseki v2 将每套主题解耦为 5 个独立层（外加 overrides），你可以任意混合两套主题的氛围与配色：'
                      : 'Every Kiseki theme separates visual styling into 5 independent layers so you can mix palettes, syntax, UI chrome, shaders, and fonts freely:'}
                  </p>
                  <div className="space-y-2 font-k-mono text-xs border-t pt-4" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 25%, transparent)' }}>
                    {COMPOSE_LAYERS.map((item) => (
                      <div key={item.layer}>
                        <span style={{ color: 'var(--k-accent-primary)' }}>{item.layer}</span> — {lang === 'zh' ? item.zh : item.en}
                      </div>
                    ))}
                  </div>
                  <CodeBlock
                    lang={lang}
                    code={
                      lang === 'zh'
                        ? `# 例：用京都夜樱的柔粉配色，搭配 8-Bit 星空的流星 Shader\nkiseki compose --palette kyoto-sakura --syntax kyoto-sakura --atmosphere starlight-8bit\n\n# 随时恢复纯正单主题\nkiseki compose --reset\n\n# 切换渲染档位（balanced 默认 / eco 省电 / ultra 满血 / static 静态 / accessibility 高对比）\nkiseki profile eco`
                        : `# Mix Kyoto Sakura palette with Starlight 8-Bit meteor shader\nkiseki compose --palette kyoto-sakura --syntax kyoto-sakura --atmosphere starlight-8bit\n\n# Reset back to pure active theme\nkiseki compose --reset\n\n# Change shader tier: static / eco / balanced / ultra / accessibility\nkiseki profile eco`
                    }
                  />
                </div>
              )}

              {activeCh.id === 'sdk' && (
                <div className="space-y-6">
                  <div className="space-y-2 border-b pb-4" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)' }}>
                    <div className="font-k-mono text-[10px] tracking-[0.2em]" style={{ color: 'var(--k-accent-primary)' }}>
                      CHAPTER 04 // THEME SDK
                    </div>
                    <h1 className="k-act-heading text-3xl m-0">{lang === 'zh' ? '制作与校验社区主题包' : 'Building Community Theme Packs'}</h1>
                  </div>
                  <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
                    {lang === 'zh'
                      ? '主题包采用纯声明式目录结构，严禁任何可执行脚本或符号链接：'
                      : 'Packs are 100% declarative JSON + validated shader templates — zero executable scripts or symlinks allowed:'}
                  </p>
                  <CodeBlock
                    lang={lang}
                    code={
                      lang === 'zh'
                        ? `my-theme/\n├── manifest.json       # 元数据、作者、LICENSE 与声明层\n├── tokens.json         # L1-L5 Semantic Tokens 与 Shader 参数\n├── LICENSE / CREDITS.md\n├── assets/             # wallpaper.png + logo.txt\n└── shaders/            # live.glsl (Ghostty) + live.hlsl (WT)`
                        : `my-theme/\n├── manifest.json       # Metadata, license & declared layers\n├── tokens.json         # L1-L5 semantic tokens & shader params\n├── LICENSE / CREDITS.md\n├── assets/             # wallpaper.png + logo.txt\n└── shaders/            # live.glsl (Ghostty) + live.hlsl (WT)`
                    }
                  />
                  <p className="text-sm leading-relaxed m-0" style={{ color: 'var(--k-text-secondary)' }}>
                    {lang === 'zh' ? (
                      <>
                        编译时由 <code className="font-k-mono">kiseki-shader</code> 自动注入{' '}
                        <code className="font-k-mono">KISEKI_BASE_BG</code>、<code className="font-k-mono">KISEKI_ACCENT_PRIMARY</code>、
                        <code className="font-k-mono">kiseki_bg_mask()</code>（保护左侧代码清晰度）与{' '}
                        <code className="font-k-mono">kiseki_narrow_factor()</code>（窄分屏淡出立绘）。开发完成后直接用内置{' '}
                        <code className="font-k-mono">naga</code> 门禁检验：
                      </>
                    ) : (
                      <>
                        Kiseki injects <code className="font-k-mono">KISEKI_BASE_BG</code>, <code className="font-k-mono">KISEKI_ACCENT_PRIMARY</code>,{' '}
                        <code className="font-k-mono">kiseki_bg_mask()</code>, and <code className="font-k-mono">kiseki_narrow_factor()</code> automatically. Validate against embedded{' '}
                        <code className="font-k-mono">naga</code> &amp; WCAG 2.1 before sharing:
                      </>
                    )}
                  </p>
                  <CodeBlock lang={lang} code={`kiseki theme validate ./my-theme\nkiseki theme install ./my-theme && kiseki use my-theme`} />
                </div>
              )}

              {activeCh.id === 'faq' && (
                <div className="space-y-6">
                  <div className="space-y-2 border-b pb-4" style={{ borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)' }}>
                    <div className="font-k-mono text-[10px] tracking-[0.2em]" style={{ color: 'var(--k-accent-primary)' }}>
                      CHAPTER 05 // ARCHITECTURE & FAQ
                    </div>
                    <h1 className="k-act-heading text-3xl m-0">{lang === 'zh' ? '架构设计与常见问题' : 'Architecture & FAQ'}</h1>
                  </div>
                  <div className="space-y-4 text-sm leading-relaxed" style={{ color: 'var(--k-text-secondary)' }}>
                    <div>
                      <div className="font-bold mb-1" style={{ color: 'var(--k-text-primary)' }}>
                        {lang === 'zh' ? '会修改我的 ls / cat 别名、Vim 键位或重装 Starship 吗？' : 'Does Kiseki override ls/cat aliases or keybindings?'}
                      </div>
                      <div>
                        {lang === 'zh'
                          ? '绝对不会。v2 只接管视觉外观层，彻底移除了所有命令别名覆盖与键位接管。'
                          : 'Never. Kiseki v2 exclusively manages the visual layer and leaves all aliases, bindings, and shell behavior untouched.'}
                      </div>
                    </div>
                    <div>
                      <div className="font-bold mb-1" style={{ color: 'var(--k-text-primary)' }}>
                        {lang === 'zh' ? '如果我用 Nix / Home Manager 管理只读 dotfiles 怎么办？' : 'How does it work with Nix / Home Manager?'}
                      </div>
                      <div>
                        {lang === 'zh' ? (
                          <>
                            遇到只读配置会自动降级，也可显式运行 <code className="font-k-mono">kiseki use &lt;id&gt; --render-only</code>{' '}
                            仅输出生成文件到 <code className="font-k-mono">generated/</code> 目录。
                          </>
                        ) : (
                          <>
                            Read-only symlinks are respected automatically, or pass <code className="font-k-mono">--render-only</code> to emit artifacts without touching host files.
                          </>
                        )}
                      </div>
                    </div>
                    <div>
                      <div className="font-bold mb-1" style={{ color: 'var(--k-text-primary)' }}>
                        {lang === 'zh' ? '如何彻底还原或从损坏状态逃生？' : 'How do I rollback, recover, or uninstall cleanly?'}
                      </div>
                      <CodeBlock
                        lang={lang}
                        code={
                          lang === 'zh'
                            ? `# 回滚上一次切换\nkiseki rollback\n\n# 紧急安全逃生（强制静态模式 + 关闭 Shader + 修复损坏状态）\nkiseki recover --safe\n\n# 彻底卸载并移除所有宿主挂载\nkiseki uninstall --purge`
                            : `# Roll back last transaction\nkiseki rollback\n\n# Safe escape hatch (static profile + shader off + auto repair)\nkiseki recover --safe\n\n# Cleanly remove all mounts and local state\nkiseki uninstall --purge`
                        }
                      />
                    </div>
                  </div>
                </div>
              )}
            </div>

            <div
              className="pt-8 border-t flex items-center justify-between gap-4 font-k-mono text-xs"
              style={{ borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)' }}
              id="k-docs-pager"
            >
              <div>
                {prevCh && (
                  <button
                    type="button"
                    onClick={() => selectWithScroll(prevCh.id)}
                    className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                    style={{ color: 'var(--k-text-secondary)' }}
                  >
                    ← {prevCh.num} {lang === 'zh' ? prevCh.titleZh : prevCh.titleEn}
                  </button>
                )}
              </div>
              <div>
                {nextCh && (
                  <button
                    type="button"
                    onClick={() => selectWithScroll(nextCh.id)}
                    className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                    style={{ color: 'var(--k-accent-primary)' }}
                  >
                    {nextCh.num} {lang === 'zh' ? nextCh.titleZh : nextCh.titleEn} →
                  </button>
                )}
              </div>
            </div>
          </article>
        </div>
      </div>
    </div>
  );
};
