import { useEffect, useRef, useState, type FC, type RefObject } from 'react';
import { KISEKI_BUNDLE } from '../data/bundle';
import { I18N, THEME_ORDER, WORLD_META } from '../data/worlds';
import type { Lang, ShaderProfile, ThemeId } from '../types/theme';

interface HeroSectionProps {
  kvCanvasRef: RefObject<HTMLCanvasElement | null>;
  fxCanvasRef: RefObject<HTMLCanvasElement | null>;
  onParallaxMove: (x: number, y: number) => void;
  onParallaxLeave: () => void;
  prefersReducedMotion: boolean;
  wipeTick: number;
  atmosphereId: ThemeId;
  typographyId: ThemeId;
  profile: ShaderProfile;
  lang: Lang;
  heroCmd: string;
  onSelectTheme: (id: ThemeId) => void;
  onToggleShader: () => void;
  onNavigate: (target: 'terminal' | 'download' | 'docs') => void;
}

export const HeroSection: FC<HeroSectionProps> = ({
  kvCanvasRef,
  fxCanvasRef,
  onParallaxMove,
  onParallaxLeave,
  prefersReducedMotion,
  wipeTick,
  atmosphereId,
  typographyId,
  profile,
  lang,
  heroCmd,
  onSelectTheme,
  onToggleShader,
  onNavigate,
}) => {
  const [copied, setCopied] = useState(false);
  const parallaxRef = useRef<HTMLDivElement | null>(null);
  const wipeRef = useRef<HTMLDivElement | null>(null);
  const lockupRef = useRef<HTMLDivElement | null>(null);

  const meta = WORLD_META[atmosphereId];
  const typoMeta = WORLD_META[typographyId];
  const t = I18N[lang];
  const isLive = profile !== 'static';

  useEffect(() => {
    if (wipeTick === 0 || prefersReducedMotion) return;
    const wipe = wipeRef.current;
    if (wipe) {
      wipe.classList.remove('is-wiping');
      void wipe.offsetWidth;
      wipe.classList.add('is-wiping');
    }
    const lockup = lockupRef.current;
    if (lockup) {
      lockup.classList.remove('k-lockup-anim');
      void lockup.offsetWidth;
      lockup.classList.add('k-lockup-anim');
    }
  }, [wipeTick, prefersReducedMotion]);

  const handleCopy = () => {
    const done = () => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1300);
    };
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(heroCmd).then(done).catch(done);
    } else {
      done();
    }
  };

  return (
    <section
      id="hero"
      className="relative min-h-screen w-full overflow-hidden flex flex-col justify-between"
      onMouseMove={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        onParallaxMove(e.clientX - rect.left, e.clientY - rect.top);
        if (prefersReducedMotion || profile === 'static' || !parallaxRef.current) return;
        const nx = (e.clientX - rect.left) / rect.width - 0.5;
        const ny = (e.clientY - rect.top) / rect.height - 0.5;
        parallaxRef.current.style.transform = `translate3d(${(-nx * 10).toFixed(1)}px, ${(-ny * 8).toFixed(1)}px, 0)`;
      }}
      onMouseLeave={() => {
        onParallaxLeave();
        if (parallaxRef.current) {
          parallaxRef.current.style.transform = 'translate3d(0, 0, 0)';
        }
      }}
    >
      {/* Parallax wrapper holding WebGL2 shader canvas + 3D perspective FX canvas */}
      <div
        id="k-kv-parallax"
        ref={parallaxRef}
        className="absolute -inset-3 transition-transform duration-300 ease-out pointer-events-none"
        style={{ background: 'var(--k-crust)' }}
      >
        <canvas ref={kvCanvasRef} id="k-kv-canvas" className="absolute inset-0 w-full h-full block" />
        <canvas ref={fxCanvasRef} id="k-fx-canvas" className="absolute inset-0 w-full h-full block pointer-events-none" />
      </div>

      {/* Cinema Viewfinder Frame & Spatial Registration Crosshairs */}
      <div
        className="pointer-events-none select-none absolute inset-3 sm:inset-5 z-10 border"
        style={{ borderColor: 'color-mix(in srgb, var(--k-text-primary) 11%, transparent)' }}
      >
        <span className="absolute -top-2 -left-1.5 font-k-mono text-xs leading-none opacity-60" style={{ color: 'var(--k-accent-primary)' }}>
          +
        </span>
        <span className="absolute -top-2 -right-1.5 font-k-mono text-xs leading-none opacity-60" style={{ color: 'var(--k-accent-primary)' }}>
          +
        </span>
        <span className="absolute -bottom-2 -left-1.5 font-k-mono text-xs leading-none opacity-60" style={{ color: 'var(--k-accent-secondary)' }}>
          +
        </span>
        <span className="absolute -bottom-2 -right-1.5 font-k-mono text-xs leading-none opacity-60" style={{ color: 'var(--k-accent-secondary)' }}>
          +
        </span>
        <div
          id="k-hero-coords"
          className="hidden md:block absolute top-14 right-5 font-k-hud text-[0.6875rem] tracking-[0.22em] opacity-65"
          style={{ color: 'var(--k-text-secondary)' }}
        >
          {meta.coords}
        </div>
      </div>

      {/* Cinema Light-Wipe Shutter Overlay */}
      <div
        id="k-scene-wipe"
        ref={wipeRef}
        className="absolute inset-0 z-20"
        style={{
          background:
            'linear-gradient(90deg, transparent, color-mix(in srgb, var(--k-accent-primary) 26%, var(--k-crust)), var(--k-crust) 55%, transparent)',
        }}
      />

      <div className="h-16 shrink-0" />

      {/* Left Negative Space: Poster Lockup */}
      <div className="relative z-10 w-full max-w-[120rem] mx-auto px-6 sm:px-12 lg:px-14 py-8 flex items-center">
        <div id="k-hero-lockup" ref={lockupRef} className="k-lockup-anim flex items-start gap-5 sm:gap-7 max-w-[34rem] lg:max-w-[38rem]">
          {/* Vertical Japanese Catchphrase + Square Hanko Seal */}
          <div className="hidden sm:flex flex-col items-center gap-3.5 pt-1 select-none shrink-0">
            <span
              id="k-hero-hanko"
              className="k-vertical-jp font-k-serif text-[0.6875rem] px-1 py-1.5 border leading-none tracking-[0.2em]"
              style={{
                borderColor: 'color-mix(in srgb, var(--k-accent-secondary) 75%, transparent)',
                color: 'var(--k-accent-secondary)',
              }}
            >
              {meta.hanko}
            </span>
            <p
              id="k-hero-vertical-jp"
              className="k-vertical-jp font-k-serif text-base sm:text-lg leading-loose m-0"
              style={{
                color: 'var(--k-text-primary)',
                textShadow: '0 2px 18px color-mix(in srgb, var(--k-accent-primary) 35%, transparent)',
              }}
            >
              {meta.verticalJp}
            </p>
            <span className="w-px h-10 opacity-45" style={{ background: 'var(--k-accent-primary)' }} />
          </div>

          {/* Horizontal Lockup */}
          <div className="space-y-4 min-w-0">
            <div className="space-y-2">
              <div className="flex items-center gap-2.5 flex-wrap">
                <span className="w-5 h-px shrink-0 opacity-75" style={{ background: 'var(--k-accent-primary)' }} />
                <span id="k-hero-katakana" className="font-k-serif text-xs tracking-[0.28em] opacity-90" style={{ color: 'var(--k-accent-secondary)' }}>
                  {meta.katakana}
                </span>
                <span id="k-hero-type-tag" className="font-k-hud text-[0.625rem] tracking-[0.2em] uppercase opacity-65" style={{ color: 'var(--k-text-muted)' }}>
                  {typoMeta.typeTag}
                </span>
              </div>

              <h1 id="k-hero-wordmark" className="m-0 k-hero-wordmark">
                <span className="k-title-sheen">KISEKI</span>
                <span className="k-wordmark-cursor k-cursor-blink" style={{ background: 'var(--k-accent-secondary)' }} />
              </h1>

              <div className="pt-0.5 max-w-[24rem]">
                <div
                  className="h-px w-full"
                  style={{ background: 'linear-gradient(90deg, color-mix(in srgb, var(--k-accent-primary) 55%, transparent), transparent)' }}
                />
                <div className="flex items-center justify-between gap-3 pt-1.5">
                  <span id="k-hero-jp-title" className="font-k-serif text-xs tracking-[0.24em] opacity-85" style={{ color: 'var(--k-text-secondary)' }}>
                    {meta.jpSubtitle}
                  </span>
                  <span className="font-k-hud text-[0.625rem] tracking-[0.2em] uppercase opacity-55 shrink-0" style={{ color: 'var(--k-text-muted)' }}>
                    RUST // LIVE SHADER
                  </span>
                </div>
              </div>

              <p className="font-k-serif sm:hidden text-sm m-0 pt-1" id="k-hero-mobile-jp" style={{ color: 'var(--k-accent-secondary)' }}>
                {meta.verticalJp}
              </p>
            </div>

            {/* 1-Sentence Instant Comprehension Copy */}
            <div className="space-y-1.5">
              <p className="text-base sm:text-lg font-medium leading-snug m-0 drop-shadow" style={{ color: 'var(--k-text-primary)' }}>
                {t.hero_sub}
              </p>
              <p className="font-k-mono text-xs m-0 opacity-75" style={{ color: 'var(--k-text-secondary)' }}>
                Ghostty · Windows Terminal · Zsh · PowerShell
              </p>
            </div>

            {/* Single Frosted Install Bar */}
            <div className="w-full max-w-[32rem] pt-1">
              <div
                className="flex items-center justify-between gap-3 px-3.5 py-2.5 border backdrop-blur-md font-k-mono text-xs"
                style={{
                  background: 'color-mix(in srgb, var(--k-crust) 72%, transparent)',
                  borderColor: 'color-mix(in srgb, var(--k-accent-primary) 42%, transparent)',
                  borderRadius: 'var(--k-radius)',
                  boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.06)',
                }}
              >
                <div className="flex items-center gap-2 overflow-x-auto whitespace-nowrap">
                  <span style={{ color: 'var(--k-accent-primary)' }}>$</span>
                  <code id="k-hero-cmd" style={{ color: 'var(--k-text-primary)' }}>
                    {heroCmd}
                  </code>
                </div>
                <button
                  id="k-hero-copy-btn"
                  type="button"
                  onClick={handleCopy}
                  className="shrink-0 bg-transparent border-0 p-0 cursor-pointer font-k-mono text-[0.6875rem] tracking-widest uppercase font-bold"
                  style={{ color: 'var(--k-accent-secondary)' }}
                >
                  {copied ? t.copied : t.copy}
                </button>
              </div>
            </div>

            {/* Direct Action Links */}
            <div className="flex flex-wrap items-center gap-5 pt-1 font-k-mono text-xs tracking-[0.12em]">
              <button
                type="button"
                onClick={() => onNavigate('download')}
                className="bg-transparent border-0 border-b p-0 pb-1 cursor-pointer font-bold flex items-center gap-1.5"
                style={{ color: 'var(--k-text-primary)', borderColor: 'var(--k-accent-primary)' }}
              >
                <span>{t.cta_dl}</span>
                <span style={{ color: 'var(--k-accent-primary)' }}>↓</span>
              </button>
              <button
                type="button"
                onClick={() => onNavigate('docs')}
                className="bg-transparent border-0 p-0 pb-1 cursor-pointer opacity-85 hover:opacity-100"
                style={{ color: 'var(--k-text-secondary)' }}
              >
                <span>{t.cta_docs}</span>
              </button>
              <button
                type="button"
                onClick={() => onNavigate('terminal')}
                className="bg-transparent border-0 p-0 pb-1 cursor-pointer opacity-80 hover:opacity-100"
                style={{ color: 'var(--k-text-muted)' }}
              >
                <span>{t.cta_try}</span>
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Hero Bottom Bar: Pre-mounted Miniature Filmstrip World Selector + 3D FX / Scroll */}
      <div className="relative z-10 w-full max-w-[120rem] mx-auto px-6 sm:px-12 lg:px-14 pb-8 flex items-end justify-between gap-6">
        <div className="space-y-2">
          <div className="font-k-hud text-[0.6875rem] tracking-[0.22em] uppercase opacity-65" style={{ color: 'var(--k-text-secondary)' }}>
            {t.world_hint}
          </div>
          <div id="k-world-selector" className="flex flex-wrap items-center gap-2.5 sm:gap-3">
            {THEME_ORDER.map((id) => {
              const m = WORLD_META[id];
              const tPack = KISEKI_BUNDLE.themes[id];
              const acc = tPack.tokens.palette.accents.primary;
              const active = atmosphereId === id;
              return (
                <button
                  key={id}
                  type="button"
                  data-select-theme={id}
                  onClick={() => onSelectTheme(id)}
                  className="group relative w-20 sm:w-24 h-12 overflow-hidden p-0 cursor-pointer border text-left transition-all"
                  style={{
                    background: tPack.tokens.palette.surfaces.base,
                    borderColor: active ? acc : 'color-mix(in srgb, var(--k-text-primary) 22%, transparent)',
                    transform: active ? 'translateY(-2px)' : 'none',
                    boxShadow: active ? `0 8px 24px -8px ${acc}` : 'none',
                  }}
                >
                  <img
                    src={tPack.thumbUrl}
                    alt={m.shortEn}
                    className="k-thumb-img absolute inset-0 w-full h-full object-cover object-right-bottom transition-transform duration-500 group-hover:scale-105"
                    style={{ opacity: active ? 0.92 : 0.48 }}
                  />
                  <div className="absolute inset-0" style={{ background: 'linear-gradient(180deg, rgba(6,6,12,0.12) 0%, rgba(6,6,12,0.82) 100%)' }} />
                  <div className="relative z-10 h-full px-2 py-1.5 flex flex-col justify-between font-k-mono leading-none">
                    <span className="k-thumb-num text-[0.625rem] tracking-widest" style={{ color: active ? acc : 'rgba(255,255,255,0.65)' }}>
                      {m.num}
                    </span>
                    <span className="k-thumb-lbl text-[0.6875rem] font-bold truncate" style={{ color: '#fff' }}>
                      {lang === 'zh' ? m.shortZh : m.shortEn}
                    </span>
                  </div>
                  <span
                    className="k-thumb-bar absolute bottom-0 left-0 right-0 h-[2px] transition-opacity"
                    style={{ background: acc, opacity: active ? 1 : 0 }}
                  />
                </button>
              );
            })}
          </div>
        </div>

        <div className="flex items-end gap-6 shrink-0">
          <button
            id="k-hero-shader-btn"
            type="button"
            onClick={onToggleShader}
            className="hidden sm:inline-flex items-center gap-2 bg-transparent border-0 p-0 cursor-pointer font-k-mono text-[0.6875rem] tracking-[0.18em] uppercase opacity-80 hover:opacity-100"
            style={{ color: 'var(--k-text-secondary)' }}
          >
            <span className="w-1.5 h-1.5 rounded-full inline-block" id="k-shader-dot" style={{ background: isLive ? 'var(--k-success)' : 'var(--k-text-muted)' }} />
            <span id="k-shader-label">{isLive ? '3D FX + SHADER: ON' : '3D FX + SHADER: OFF'}</span>
          </button>

          <a
            href="#terminal"
            className="flex flex-col items-center gap-2 no-underline opacity-75 hover:opacity-100"
            style={{ color: 'var(--k-text-secondary)' }}
          >
            <span className="font-k-mono text-[0.625rem] tracking-[0.3em] uppercase" style={{ writingMode: 'vertical-rl' }}>
              SCROLL
            </span>
            <span className="w-px h-9 relative overflow-hidden block" style={{ background: 'color-mix(in srgb, var(--k-text-muted) 35%, transparent)' }}>
              <span className="w-full h-full block k-scroll-line" style={{ background: 'var(--k-accent-primary)' }} />
            </span>
          </a>
        </div>
      </div>

      <div
        className="absolute bottom-0 left-0 right-0 h-[2px] z-10 pointer-events-none"
        style={{
          background: 'linear-gradient(90deg, transparent, var(--k-accent-primary) 28%, var(--k-accent-secondary) 72%, transparent)',
          opacity: 0.65,
        }}
      />
    </section>
  );
};
