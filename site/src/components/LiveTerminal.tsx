import { useEffect, useRef, useState, type FC, type FormEvent, type RefObject } from 'react';
import { KISEKI_BUNDLE } from '../data/bundle';
import { I18N, THEME_ORDER, WORLD_META } from '../data/worlds';
import type { Lang, LayerState, ShaderProfile, ThemeId } from '../types/theme';

export interface TermLogLine {
  id: number;
  text: string;
  tone: 'cmd' | 'ok' | 'info' | 'warn' | 'err' | 'muted' | 'primary';
  promptSym?: string;
}

interface LiveTerminalProps {
  termCanvasRef: RefObject<HTMLCanvasElement | null>;
  inputRef: RefObject<HTMLInputElement | null>;
  prefersReducedMotion: boolean;
  activeTheme: ThemeId;
  layers: LayerState;
  profile: ShaderProfile;
  txCounter: number;
  narrowSplit: boolean;
  lang: Lang;
  pickerActive: boolean;
  pickerIndex: number;
  logLines: TermLogLine[];
  onExecuteCmd: (raw: string) => void;
  onSelectTheme: (id: ThemeId, withWipe?: boolean) => void;
  onPickIndex: (idx: number) => void;
  onToggleSplit: () => void;
  onCycleShader: () => void;
}

const TONE_COLOR: Record<TermLogLine['tone'], string> = {
  cmd: 'var(--k-text-primary)',
  ok: 'var(--k-success)',
  info: 'var(--k-accent-secondary)',
  warn: 'var(--k-warning)',
  err: 'var(--k-error)',
  muted: 'var(--k-text-muted)',
  primary: 'var(--k-accent-primary)',
};

export const LiveTerminal: FC<LiveTerminalProps> = ({
  termCanvasRef,
  inputRef,
  prefersReducedMotion,
  activeTheme,
  layers,
  profile,
  txCounter,
  narrowSplit,
  lang,
  pickerActive,
  pickerIndex,
  logLines,
  onExecuteCmd,
  onSelectTheme,
  onPickIndex,
  onToggleSplit,
  onCycleShader,
}) => {
  const [cmdInput, setCmdInput] = useState('');
  const shellRef = useRef<HTMLDivElement | null>(null);
  const sheenRef = useRef<HTMLDivElement | null>(null);
  const logBoxRef = useRef<HTMLDivElement | null>(null);

  const meta = WORLD_META[layers.atmosphere];
  const palPack = KISEKI_BUNDLE.themes[layers.palette];
  const synPack = KISEKI_BUNDLE.themes[layers.syntax];
  const uiPack = KISEKI_BUNDLE.themes[layers.ui];
  const atmPack = KISEKI_BUNDLE.themes[layers.atmosphere];

  const promptCfg = uiPack.tokens.overrides.prompt;
  const ffCfg = uiPack.tokens.overrides.fastfetch;
  const ansi = palPack.tokens.palette.ansi;
  const syn = synPack.tokens.syntax;
  const txt = palPack.tokens.palette.text;
  const t = I18N[lang];

  const isComposed = new Set(Object.values(layers)).size > 1;
  let composeBadge = `${activeTheme} · ${profile}`;
  if (isComposed) {
    const parts = [`L1:${layers.palette}`, `L4:${layers.atmosphere}`];
    if (
      (layers.typography !== layers.palette && layers.typography !== layers.atmosphere) ||
      (layers.palette === layers.atmosphere && layers.typography !== layers.palette)
    ) {
      parts.push(`L5:${layers.typography}`);
    }
    composeBadge = parts.join(' × ');
  }

  const ffVals: Record<string, string> = {
    title: 'pilot@kiseki-v2',
    os: 'macOS / Windows 11',
    shell: 'zsh (0-fork)',
    terminal: 'Ghostty · WebGL2',
    cpu: 'Rust 8.18 MiB',
    memory: `state.gen = ${txCounter}`,
    uptime: `profile: ${profile}`,
  };

  const swatches = [ansi.red, ansi.yellow, ansi.green, ansi.cyan, ansi.blue, ansi.magenta];

  useEffect(() => {
    if (logBoxRef.current) {
      logBoxRef.current.scrollTop = logBoxRef.current.scrollHeight;
    }
  }, [logLines]);

  useEffect(() => {
    if (narrowSplit && shellRef.current) {
      shellRef.current.style.transform = '';
    }
  }, [narrowSplit]);

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    const val = cmdInput;
    setCmdInput('');
    onExecuteCmd(val);
  };

  return (
    <section id="terminal" className="relative z-10 py-24 sm:py-32 overflow-hidden">
      <div className="pointer-events-none absolute inset-0 k-blueprint-wash" />
      <div
        className="pointer-events-none absolute -top-24 left-1/2 -translate-x-1/2 w-[780px] h-[360px] rounded-full blur-3xl opacity-[0.07]"
        style={{ background: 'radial-gradient(circle, var(--k-accent-primary), transparent 70%)' }}
      />

      <div
        id="k-act1-watermark"
        className="k-watermark pointer-events-none select-none absolute top-10 left-1/2 text-[88px] sm:text-[160px] lg:text-[210px] leading-none whitespace-nowrap"
        style={{ color: 'var(--k-text-primary)', opacity: 0.032 }}
      >
        {meta.watermark}
      </div>

      <div className="k-reveal relative z-10 max-w-6xl mx-auto px-6 sm:px-10 space-y-10">
        <div
          className="max-w-4xl mx-auto flex items-end justify-between gap-6 border-b pb-5"
          style={{ borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)' }}
        >
          <div className="space-y-2">
            <div className="font-k-hud text-[11px] tracking-[0.26em] uppercase" style={{ color: 'var(--k-accent-primary)' }}>
              01 // LIVE DEMO
            </div>
            <h2 className="k-act-heading text-2xl sm:text-4xl m-0" style={{ color: 'var(--k-text-primary)' }}>
              {t.act1_title}
            </h2>
            <p className="text-xs sm:text-sm m-0" style={{ color: 'var(--k-text-muted)' }}>
              {t.act1_desc}
            </p>
          </div>
          <div className="hidden sm:flex items-center shrink-0 select-none">
            <span
              className="k-vertical-jp font-k-serif text-xs px-1.5 py-2 border opacity-75"
              style={{
                borderColor: 'color-mix(in srgb, var(--k-border) 50%, transparent)',
                color: 'var(--k-accent-secondary)',
              }}
            >
              第壱幕・実機
            </span>
          </div>
        </div>

        <div className="flex flex-col items-center space-y-4" style={{ perspective: '1200px' }}>
          <div
            id="k-term-shell"
            ref={shellRef}
            onMouseMove={(e) => {
              if (prefersReducedMotion || narrowSplit || document.activeElement === inputRef.current) return;
              if (!shellRef.current || !sheenRef.current) return;
              const r = shellRef.current.getBoundingClientRect();
              const x = (e.clientX - r.left) / r.width;
              const y = (e.clientY - r.top) / r.height;
              const rx = ((0.5 - y) * 3.5).toFixed(2);
              const ry = ((x - 0.5) * 4.0).toFixed(2);
              shellRef.current.style.transform = `rotateX(${rx}deg) rotateY(${ry}deg)`;
              sheenRef.current.style.opacity = '1';
              sheenRef.current.style.background = `radial-gradient(360px circle at ${(x * 100).toFixed(1)}% ${(y * 100).toFixed(1)}%, color-mix(in srgb, var(--k-accent-primary) 12%, transparent), transparent 70%)`;
            }}
            onMouseLeave={() => {
              if (shellRef.current) shellRef.current.style.transform = '';
              if (sheenRef.current) sheenRef.current.style.opacity = '0';
            }}
            className={`k-terminal-bezel relative w-full ${narrowSplit ? 'max-w-sm' : 'max-w-4xl'} border overflow-hidden transition-all duration-300`}
            style={{
              borderColor: 'color-mix(in srgb, var(--k-border) 55%, transparent)',
              borderRadius: 'var(--k-radius)',
              background: 'var(--k-base)',
            }}
          >
            <div ref={sheenRef} id="k-term-sheen" className="pointer-events-none absolute inset-0 z-20 opacity-0 transition-opacity duration-300" />

            {/* Titlebar */}
            <div
              className="relative flex items-center justify-between px-4 py-2.5 border-b font-k-mono text-[11px]"
              style={{
                background: 'var(--k-tab-bar-bg)',
                borderColor: 'color-mix(in srgb, var(--k-border) 42%, transparent)',
              }}
            >
              <div className="flex items-center gap-3 min-w-0">
                <div className="flex items-center gap-1.5 shrink-0">
                  <span className="w-2.5 h-2.5 rounded-full inline-block" style={{ background: 'var(--k-error)' }} />
                  <span className="w-2.5 h-2.5 rounded-full inline-block" style={{ background: 'var(--k-warning)' }} />
                  <span className="w-2.5 h-2.5 rounded-full inline-block" style={{ background: 'var(--k-success)' }} />
                </div>
                <span id="k-term-title" className="truncate" style={{ color: 'var(--k-text-secondary)' }}>
                  {promptCfg.consoleTitlePrefix} // ~/projects/kiseki-terminal
                </span>
              </div>
              <span id="k-term-compose-badge" className="hidden sm:inline text-[10px]" style={{ color: 'var(--k-accent-primary)' }}>
                {composeBadge}
              </span>
              <span className="absolute bottom-0 left-0 w-36 h-[1px]" style={{ background: 'var(--k-accent-primary)' }} />
            </div>

            {/* WebGL2 Live Shader Terminal Body */}
            <div className="relative min-h-[420px] flex flex-col justify-between overflow-hidden" style={{ background: 'var(--k-base)' }}>
              <canvas ref={termCanvasRef} id="k-term-canvas" className="absolute inset-0 w-full h-full block" />

              <div className="relative z-10 p-5 sm:p-6 flex-1 flex flex-col justify-between gap-4 font-k-mono text-xs">
                <div className="space-y-4">
                  <div
                    className="flex flex-col sm:flex-row sm:items-start gap-4 pb-3 border-b max-w-[92%]"
                    style={{ borderColor: 'color-mix(in srgb, var(--k-border) 28%, transparent)' }}
                  >
                    <div
                      id="k-term-ansi-logo"
                      className="k-ansi-logo shrink-0"
                      dangerouslySetInnerHTML={{ __html: atmPack.logoHtml }}
                    />
                    <div className="min-w-0 space-y-1 text-[11px] leading-snug">
                      <div id="k-ff-header" className="font-bold tracking-wide" style={{ color: 'var(--k-accent-primary)' }}>
                        {ffCfg.headerTitle}
                      </div>
                      <div id="k-ff-fields" className="space-y-0.5">
                        {Object.entries(ffCfg.fieldLabels)
                          .slice(0, 5)
                          .map(([k, lbl]) => (
                            <div key={k} className="flex items-center gap-2">
                              <span className="w-16 shrink-0" style={{ color: 'var(--k-accent-secondary)' }}>
                                {lbl}
                              </span>
                              <span className="truncate" style={{ color: 'var(--k-text-primary)' }}>
                                {ffVals[k] ?? ''}
                              </span>
                            </div>
                          ))}
                      </div>
                      <div id="k-ff-swatches" className="flex items-center gap-0.5 pt-1">
                        {swatches.map((hex, i) => (
                          <span key={i} className="inline-block w-3 h-2" style={{ background: hex }} />
                        ))}
                      </div>
                    </div>
                  </div>

                  <div
                    className="p-3 border text-[11px] leading-relaxed max-w-[90%] sm:max-w-[60%]"
                    style={{
                      background: 'color-mix(in srgb, var(--k-crust) 72%, transparent)',
                      borderColor: 'color-mix(in srgb, var(--k-border) 32%, transparent)',
                      borderRadius: 'var(--k-radius)',
                    }}
                  >
                    <pre id="k-term-code" className="m-0 whitespace-pre overflow-x-auto">
                      <span style={{ color: syn.comment }}>// 5-layer semantic token sync (&lt;1ms)</span>
                      {'\n'}
                      <span style={{ color: syn.keyword }}>pub fn</span>{' '}
                      <span style={{ color: syn.function }}>resonate</span>(
                      <span style={{ color: txt.primary }}>theme</span>: &amp;
                      <span style={{ color: syn.type }}>str</span>){' '}
                      <span style={{ color: syn.operator }}>-&gt;</span>{' '}
                      <span style={{ color: syn.type }}>String</span> {'{\n'}
                      {'    '}
                      <span style={{ color: syn.function }}>format!</span>(
                      <span style={{ color: syn.string }}>&quot;★ {'{'}theme{'}'} synced&quot;</span>)
                      {'\n}'}
                    </pre>
                  </div>

                  <div ref={logBoxRef} id="k-term-log" className="k-term-scroll max-h-24 overflow-y-auto space-y-1 text-[11px] max-w-[92%] sm:max-w-[68%]">
                    {logLines.map((line) => (
                      <div key={line.id}>
                        {line.tone === 'cmd' ? (
                          <div style={{ color: 'var(--k-text-muted)' }}>
                            {line.promptSym ?? '›'} <span style={{ color: 'var(--k-text-primary)' }}>{line.text}</span>
                          </div>
                        ) : (
                          <div style={{ color: TONE_COLOR[line.tone] }}>{line.text}</div>
                        )}
                      </div>
                    ))}
                  </div>

                  {pickerActive && (
                    <div
                      id="k-term-picker"
                      className="border p-3 max-w-md space-y-1.5"
                      style={{
                        background: 'color-mix(in srgb, var(--k-crust) 92%, transparent)',
                        borderColor: 'var(--k-accent-primary)',
                        borderRadius: 'var(--k-radius)',
                      }}
                    >
                      <div
                        className="flex items-center justify-between text-[10px] pb-1 border-b"
                        style={{
                          borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)',
                          color: 'var(--k-accent-primary)',
                        }}
                      >
                        <span>kiseki pick // Ratatui</span>
                        <span style={{ color: 'var(--k-text-muted)' }}>[↑↓] 预览 · [Enter] 应用 · [Esc] 取消</span>
                      </div>
                      <div id="k-term-picker-list" className="space-y-0.5 text-[11px]">
                        {THEME_ORDER.map((id, idx) => {
                          const sel = idx === pickerIndex;
                          const m = WORLD_META[id];
                          return (
                            <div
                              key={id}
                              onClick={() => onPickIndex(idx)}
                              className="px-2 py-1 flex items-center justify-between cursor-pointer"
                              style={{
                                background: sel ? 'var(--k-surface1)' : 'transparent',
                                color: sel ? 'var(--k-accent-primary)' : 'var(--k-text-secondary)',
                              }}
                            >
                              <span>
                                {sel ? '▸ ' : '  '}
                                {m.num} {id}
                              </span>
                              <span style={{ color: 'var(--k-accent-secondary)' }}>
                                {KISEKI_BUNDLE.themes[id].tokens.overrides.prompt.preset}
                              </span>
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  )}
                </div>

                <form
                  id="k-term-form"
                  onSubmit={handleSubmit}
                  className="pt-2.5 border-t flex items-center gap-2 max-w-full sm:max-w-[72%]"
                  style={{ borderColor: 'color-mix(in srgb, var(--k-border) 32%, transparent)' }}
                >
                  <span
                    id="k-prompt-badge"
                    className="px-1.5 py-0.5 text-[10px] font-bold shrink-0"
                    style={{
                      borderRadius: '2px',
                      background: promptCfg.segmentColors.badgeBg,
                      color: promptCfg.segmentColors.badgeFg,
                    }}
                  >
                    {promptCfg.badgeText}
                  </span>
                  <span
                    id="k-prompt-path"
                    className="px-1.5 py-0.5 text-[10px] shrink-0 hidden sm:inline"
                    style={{
                      borderRadius: '2px',
                      background: promptCfg.segmentColors.pathBg,
                      color: promptCfg.segmentColors.pathFg,
                    }}
                  >
                    ~/kiseki-terminal
                  </span>
                  <span id="k-prompt-sym" className="font-bold shrink-0" style={{ color: 'var(--k-accent-primary)' }}>
                    {promptCfg.promptSymbol}
                  </span>
                  <input
                    ref={inputRef}
                    id="k-term-input"
                    type="text"
                    value={cmdInput}
                    onChange={(e) => setCmdInput(e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="输入 kiseki pick / use nerv-eva / compose / rollback / help..."
                    className="w-full bg-transparent border-0 outline-none font-k-mono text-xs"
                    style={{ color: 'var(--k-text-primary)' }}
                  />
                </form>
              </div>
            </div>
          </div>

          {/* Glass-Hairline Dock Bar */}
          <div
            className="w-full max-w-4xl px-4 py-2.5 border backdrop-blur-sm flex flex-wrap items-center justify-between gap-4 font-k-mono text-[11px]"
            style={{
              background: 'color-mix(in srgb, var(--k-mantle) 65%, transparent)',
              borderColor: 'color-mix(in srgb, var(--k-border) 30%, transparent)',
              borderRadius: 'var(--k-radius)',
              color: 'var(--k-text-muted)',
            }}
          >
            <div className="flex flex-wrap items-center gap-4">
              <div
                className="flex items-center gap-1.5 pr-3 border-r"
                id="k-dock-dots"
                style={{ borderColor: 'color-mix(in srgb, var(--k-border) 35%, transparent)' }}
              >
                {THEME_ORDER.map((id) => {
                  const acc = KISEKI_BUNDLE.themes[id].tokens.palette.accents.primary;
                  const active = layers.atmosphere === id;
                  return (
                    <button
                      key={id}
                      type="button"
                      title={id}
                      onClick={() => onSelectTheme(id, true)}
                      className="w-2.5 h-2.5 rounded-full border-0 p-0 cursor-pointer transition-transform"
                      style={{
                        background: acc,
                        transform: active ? 'scale(1.25)' : 'scale(1)',
                        opacity: active ? 1 : 0.65,
                        boxShadow: active ? `0 0 0 2px var(--k-crust), 0 0 0 3px ${acc}` : 'none',
                      }}
                    />
                  );
                })}
              </div>
              <button
                type="button"
                onClick={() => {
                  inputRef.current?.focus();
                  onExecuteCmd('kiseki pick');
                }}
                className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                style={{ color: 'var(--k-text-secondary)' }}
              >
                kiseki pick
              </button>
              <button
                type="button"
                onClick={() => {
                  inputRef.current?.focus();
                  onExecuteCmd('kiseki compose --palette kyoto-sakura --atmosphere starlight-8bit');
                }}
                className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                style={{ color: 'var(--k-accent-primary)' }}
              >
                {t.try_mix1}
              </button>
              <button
                type="button"
                onClick={() => {
                  inputRef.current?.focus();
                  onExecuteCmd('kiseki compose --palette nerv-eva --atmosphere shinkai-rain');
                }}
                className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                style={{ color: 'var(--k-accent-secondary)' }}
              >
                {t.try_mix2}
              </button>
              <button
                type="button"
                onClick={() => {
                  inputRef.current?.focus();
                  onExecuteCmd('kiseki rollback');
                }}
                className="bg-transparent border-0 p-0 cursor-pointer hover:underline"
                style={{ color: 'var(--k-text-secondary)' }}
              >
                rollback
              </button>
            </div>

            <div className="flex items-center gap-5">
              <button
                id="k-split-toggle"
                type="button"
                onClick={onToggleSplit}
                className="bg-transparent border-0 p-0 cursor-pointer pb-0.5 border-b transition-colors"
                style={{
                  color: 'var(--k-text-secondary)',
                  borderColor: 'color-mix(in srgb, var(--k-border) 50%, transparent)',
                }}
              >
                <span>{narrowSplit ? t.split_narrow : t.split_wide}</span>
              </button>
              <button
                id="k-term-shader-toggle"
                type="button"
                onClick={onCycleShader}
                className="bg-transparent border-0 p-0 cursor-pointer pb-0.5 border-b transition-colors"
                style={{ color: 'var(--k-accent-primary)', borderColor: 'var(--k-accent-primary)' }}
              >
                Shader: {profile}
              </button>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
};
