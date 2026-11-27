import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type FC } from 'react';
import { BootCurtain } from './components/BootCurtain';
import { DocsHub } from './components/DocsHub';
import { Header } from './components/Header';
import { HeroSection } from './components/HeroSection';
import { InstallSection } from './components/InstallSection';
import { LiveTerminal, type TermLogLine } from './components/LiveTerminal';
import { PromisesSection } from './components/PromisesSection';
import { KISEKI_BUNDLE } from './data/bundle';
import { DOC_CHAPTERS_META } from './data/docs';
import {
  DEFAULT_OPENING_THEME,
  INSTALL_SNIPPETS_BY_REGION,
  THEME_ORDER,
  defaultInstallTabForOS,
  detectInstallRegion,
  detectOS,
} from './data/worlds';
import { Kiseki3DStage } from './engine/perspectiveFx';
import { KisekiShaderStage, getWallpaperImage } from './engine/webglShader';
import type {
  DocChapterId,
  HistorySnapshot,
  InstallRegion,
  InstallTab,
  Lang,
  LayerState,
  ShaderProfile,
  ThemeId,
  ViewMode,
} from './types/theme';

const ALL_LAYERS_DEFAULT: LayerState = {
  palette: DEFAULT_OPENING_THEME,
  syntax: DEFAULT_OPENING_THEME,
  ui: DEFAULT_OPENING_THEME,
  atmosphere: DEFAULT_OPENING_THEME,
  typography: DEFAULT_OPENING_THEME,
};

function parseHashRoute(): { view: ViewMode; docChapter?: DocChapterId } {
  if (typeof window === 'undefined') return { view: 'landing' };
  const h = window.location.hash.replace(/^#/, '');
  if (h.startsWith('docs')) {
    const sub = h.split('/')[1] as DocChapterId | undefined;
    const valid = DOC_CHAPTERS_META.some((c) => c.id === sub);
    return { view: 'docs', docChapter: valid && sub ? sub : 'quickstart' };
  }
  return { view: 'landing' };
}

export const App: FC = () => {
  const prefersReducedMotion = useMemo(
    () => typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches,
    [],
  );
  const detectedOS = useMemo(() => detectOS(), []);
  const initialRoute = useMemo(() => parseHashRoute(), []);

  const [bootDone, setBootDone] = useState(prefersReducedMotion);
  const [scrolled, setScrolled] = useState(false);
  const [view, setView] = useState<ViewMode>(initialRoute.view);
  const [lang, setLang] = useState<Lang>('zh');
  const [activeTheme, setActiveTheme] = useState<ThemeId>(DEFAULT_OPENING_THEME);
  const [layers, setLayers] = useState<LayerState>(ALL_LAYERS_DEFAULT);
  const [profile, setProfile] = useState<ShaderProfile>(prefersReducedMotion ? 'static' : 'balanced');
  const [bgOpacityOverride, setBgOpacityOverride] = useState<number | null>(null);
  const [narrowSplit, setNarrowSplit] = useState(false);
  const [txCounter, setTxCounter] = useState(1);
  const [wipeTick, setWipeTick] = useState(0);

  const [pickerActive, setPickerActive] = useState(false);
  const [pickerIndex, setPickerIndex] = useState(0);
  const pickerBackupRef = useRef<{ activeTheme: ThemeId; layers: LayerState } | null>(null);
  const historyStackRef = useRef<HistorySnapshot[]>([]);

  const [installTab, setInstallTab] = useState<InstallTab>(() => defaultInstallTabForOS(detectedOS));
  const [installRegion, setInstallRegion] = useState<InstallRegion>(() => detectInstallRegion());
  const [docChapter, setDocChapter] = useState<DocChapterId>(initialRoute.docChapter ?? 'quickstart');
  const [docSearch, setDocSearch] = useState('');

  const nextLogId = useRef(2);
  const [logLines, setLogLines] = useState<TermLogLine[]>([
    {
      id: 1,
      tone: 'muted',
      text: 'Kiseki v2 ready — type help or press 1–4 to switch themes.',
    },
  ]);

  const kvCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const fxCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const termCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const termInputRef = useRef<HTMLInputElement | null>(null);

  const kvStageRef = useRef<KisekiShaderStage | null>(null);
  const termStageRef = useRef<KisekiShaderStage | null>(null);
  const fxStageRef = useRef<Kiseki3DStage | null>(null);

  const viewRef = useRef(view);
  viewRef.current = view;

  const appendLog = useCallback((line: Omit<TermLogLine, 'id'>) => {
    const id = nextLogId.current++;
    setLogLines((prev) => [...prev.slice(-9), { ...line, id }]);
  }, []);

  const pushSnapshot = useCallback(
    (snap: HistorySnapshot) => {
      historyStackRef.current.push(snap);
      if (historyStackRef.current.length > 16) historyStackRef.current.shift();
    },
    [],
  );

  // Build scoped CSS custom properties from active palette & UI layers
  const appCssVars = useMemo(() => {
    const p = KISEKI_BUNDLE.themes[layers.palette].tokens.palette;
    const ui = KISEKI_BUNDLE.themes[layers.ui].tokens.ui;
    return {
      '--k-crust': p.surfaces.crust,
      '--k-mantle': p.surfaces.mantle,
      '--k-base': p.surfaces.base,
      '--k-surface0': p.surfaces.surface0,
      '--k-surface1': p.surfaces.surface1,
      '--k-overlay0': p.surfaces.overlay0,
      '--k-text-primary': p.text.primary,
      '--k-text-secondary': p.text.secondary,
      '--k-text-muted': p.text.muted,
      '--k-text-inverse': p.text.inverse,
      '--k-accent-primary': p.accents.primary,
      '--k-accent-secondary': p.accents.secondary,
      '--k-accent-tertiary': p.accents.tertiary,
      '--k-error': p.semantic.error,
      '--k-warning': p.semantic.warning,
      '--k-success': p.semantic.success,
      '--k-border': ui.border,
      '--k-panel-bg': ui.panelBg,
      '--k-tab-bar-bg': ui.tabBarBg,
      '--k-tab-active-bg': ui.tabActiveBg,
    } as CSSProperties;
  }, [layers.palette, layers.ui]);

  // Initialize GPU WebGL2 + 3D FX engines + staggered idle pre-warming
  useEffect(() => {
    const kvCanvas = kvCanvasRef.current;
    const fxCanvas = fxCanvasRef.current;
    const termCanvas = termCanvasRef.current;
    if (!kvCanvas || !fxCanvas || !termCanvas) return;

    const isLanding = () => viewRef.current === 'landing';
    const kvStage = new KisekiShaderStage(kvCanvas, 'kv', KISEKI_BUNDLE, isLanding);
    const termStage = new KisekiShaderStage(termCanvas, 'terminal', KISEKI_BUNDLE, isLanding);
    const fxStage = new Kiseki3DStage(fxCanvas, KISEKI_BUNDLE, prefersReducedMotion, isLanding);

    kvStageRef.current = kvStage;
    termStageRef.current = termStage;
    fxStageRef.current = fxStage;

    const bootTimer = window.setTimeout(() => {
      setBootDone(true);
      const others = THEME_ORDER.filter((id) => id !== DEFAULT_OPENING_THEME);
      others.forEach((id, idx) => {
        window.setTimeout(() => {
          getWallpaperImage(id, KISEKI_BUNDLE, () => {
            kvStage.prewarm(id, profile);
          });
        }, 160 * (idx + 1));
      });
    }, prefersReducedMotion ? 0 : 620);

    return () => {
      window.clearTimeout(bootTimer);
      kvStage.dispose();
      termStage.dispose();
      fxStage.dispose();
      kvStageRef.current = null;
      termStageRef.current = null;
      fxStageRef.current = null;
    };
    // Mount once for the app lifetime
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Push GPU state updates whenever visual layers, profile, opacity, or view changes
  useEffect(() => {
    const kv = kvStageRef.current;
    const term = termStageRef.current;
    const fx = fxStageRef.current;
    if (!kv || !term || !fx) return;

    if (view === 'docs') {
      kv.stopLoop();
      term.stopLoop();
      fx.stopLoop();
      return;
    }

    kv.update(layers.palette, layers.atmosphere, profile, null);
    term.update(layers.palette, layers.atmosphere, profile, bgOpacityOverride);
    fx.configure(layers.atmosphere, profile !== 'static');
  }, [layers.palette, layers.atmosphere, profile, bgOpacityOverride, view]);

  // Sync terminal canvas size after narrow/wide split CSS transition finishes
  useEffect(() => {
    const id = window.setTimeout(() => {
      termStageRef.current?.syncSize();
    }, 310);
    return () => window.clearTimeout(id);
  }, [narrowSplit]);

  // Scroll spy & header blur
  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 48);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  // Section scroll reveal observer when landing view is active
  useEffect(() => {
    if (view !== 'landing') return;
    const els = document.querySelectorAll('.k-reveal');
    if (!('IntersectionObserver' in window) || prefersReducedMotion) {
      els.forEach((el) => el.classList.add('is-revealed'));
      return;
    }
    const obs = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add('is-revealed');
            obs.unobserve(entry.target);
          }
        }
      },
      { threshold: 0.12 },
    );
    els.forEach((el) => obs.observe(el));
    return () => obs.disconnect();
  }, [view, prefersReducedMotion]);

  const activateTheme = useCallback(
    (themeId: ThemeId, recordTx = true, withWipe = true) => {
      if (!(themeId in KISEKI_BUNDLE.themes)) return;
      if (recordTx) {
        pushSnapshot({
          activeTheme,
          layers: { ...layers },
          profile,
          bgOpacityOverride,
        });
        setTxCounter((c) => c + 1);
      }
      if (withWipe && themeId !== activeTheme) {
        setWipeTick((w) => w + 1);
        fxStageRef.current?.triggerBurst();
      }
      setActiveTheme(themeId);
      setLayers({
        palette: themeId,
        syntax: themeId,
        ui: themeId,
        atmosphere: themeId,
        typography: themeId,
      });
      setBgOpacityOverride(null);
    },
    [activeTheme, layers, profile, bgOpacityOverride, pushSnapshot],
  );

  const navigateTo = useCallback((target: 'home' | 'terminal' | 'download' | 'docs', docId?: DocChapterId) => {
    if (target === 'docs') {
      const nextDoc = docId ?? docChapter;
      setView('docs');
      if (docId) setDocChapter(docId);
      window.history.replaceState(null, '', `#docs/${nextDoc}`);
      window.scrollTo({ top: 0, behavior: 'auto' });
    } else {
      setView('landing');
      window.history.replaceState(null, '', target === 'home' ? '#' : `#${target}`);
      if (target === 'home') {
        window.scrollTo({ top: 0, behavior: 'smooth' });
      } else {
        window.setTimeout(() => {
          document.getElementById(target)?.scrollIntoView({ behavior: 'smooth' });
        }, 30);
      }
    }
  }, [docChapter]);

  const executeCmd = useCallback(
    (raw: string) => {
      const cmd = raw.trim();
      if (!cmd) return;
      const sym = KISEKI_BUNDLE.themes[layers.ui].tokens.overrides.prompt.promptSymbol;
      appendLog({ tone: 'cmd', text: cmd, promptSym: sym });

      const parts = cmd.split(/\s+/);
      const head = parts[0] === 'kiseki' ? parts.slice(1) : parts;
      const sub = (head[0] ?? '').toLowerCase();

      if (sub === 'clear') {
        setLogLines([]);
        return;
      }
      if (sub === 'help' || sub === '?') {
        appendLog({
          tone: 'primary',
          text: 'use <id> | pick | compose --palette <id> --atmosphere <id> --typography <id> | compose --reset | live <on|off> | bg <0..1|reset> | rollback | doctor | clear',
        });
        return;
      }
      if (sub === 'use' && head[1]) {
        const targetId = head[1] as ThemeId;
        if (targetId in KISEKI_BUNDLE.themes) {
          const nextTx = txCounter + 1;
          activateTheme(targetId, true, false);
          appendLog({ tone: 'ok', text: `✓ tx #${String(nextTx).padStart(3, '0')}: mounted ${targetId}` });
        } else {
          appendLog({ tone: 'err', text: `✗ unknown theme '${head[1]}'` });
        }
        return;
      }
      if (sub === 'pick') {
        pickerBackupRef.current = { activeTheme, layers: { ...layers } };
        setPickerIndex(Math.max(0, THEME_ORDER.indexOf(activeTheme)));
        setPickerActive(true);
        appendLog({ tone: 'info', text: '● Picker open — ↑/↓ preview, Enter apply, Esc cancel.' });
        return;
      }
      if (sub === 'compose') {
        if (head.includes('--reset')) {
          activateTheme(activeTheme, true, false);
          appendLog({ tone: 'ok', text: '✓ composition reset' });
          return;
        }
        pushSnapshot({ activeTheme, layers: { ...layers }, profile, bgOpacityOverride });
        setTxCounter((c) => c + 1);
        const nextLayers: LayerState = { ...layers };
        for (let i = 1; i < head.length; i += 2) {
          const k = (head[i] ?? '').replace(/^--/, '') as keyof LayerState;
          const v = head[i + 1] as ThemeId;
          if (k in nextLayers && v && v in KISEKI_BUNDLE.themes) {
            nextLayers[k] = v;
          }
        }
        setLayers(nextLayers);
        appendLog({ tone: 'ok', text: `✓ composed L1(${nextLayers.palette}) × L4(${nextLayers.atmosphere}) × L5(${nextLayers.typography})` });
        return;
      }
      if (sub === 'live') {
        const arg = (head[1] ?? '').toLowerCase();
        const nextProfile: ShaderProfile = arg === 'off' ? 'static' : 'balanced';
        setProfile(nextProfile);
        appendLog({ tone: 'ok', text: `✓ shader -> ${nextProfile}` });
        return;
      }
      if (sub === 'bg' && head[1]) {
        const nextOpacity = head[1] === 'reset' ? null : Math.max(0, Math.min(1, parseFloat(head[1]) || 0));
        setBgOpacityOverride(nextOpacity);
        appendLog({ tone: 'ok', text: '✓ wallpaper opacity updated' });
        return;
      }
      if (sub === 'rollback') {
        const prev = historyStackRef.current.pop();
        if (prev) {
          setActiveTheme(prev.activeTheme);
          setLayers(prev.layers);
          setProfile(prev.profile);
          setBgOpacityOverride(prev.bgOpacityOverride);
          setTxCounter((c) => c + 1);
          appendLog({ tone: 'warn', text: `↺ rolled back to ${prev.activeTheme}` });
        } else {
          appendLog({ tone: 'muted', text: 'already at initial snapshot' });
        }
        return;
      }
      if (sub === 'doctor') {
        appendLog({ tone: 'ok', text: '[ok] schema v1 · [ok] naga GLSL/HLSL · [ok] WCAG 2.1 · [ok] 0-fork hook (54.7µs)' });
        return;
      }
      appendLog({ tone: 'err', text: `unknown command '${cmd}'. Type help.` });
    },
    [activeTheme, layers, profile, bgOpacityOverride, txCounter, activateTheme, appendLog, pushSnapshot],
  );

  // Global keyboard shortcuts (1-4 theme switching + Ratatui picker navigation)
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (pickerActive) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          const next = (pickerIndex + 1) % THEME_ORDER.length;
          setPickerIndex(next);
          activateTheme(THEME_ORDER[next]!, false, false);
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          const next = (pickerIndex - 1 + THEME_ORDER.length) % THEME_ORDER.length;
          setPickerIndex(next);
          activateTheme(THEME_ORDER[next]!, false, false);
          return;
        }
        if (e.key === 'Enter') {
          e.preventDefault();
          const chosen = THEME_ORDER[pickerIndex]!;
          setPickerActive(false);
          activateTheme(chosen, true, false);
          appendLog({ tone: 'ok', text: `✓ picked ${chosen}` });
          return;
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          setPickerActive(false);
          if (pickerBackupRef.current) {
            setActiveTheme(pickerBackupRef.current.activeTheme);
            setLayers(pickerBackupRef.current.layers);
          }
          appendLog({ tone: 'muted', text: 'picker cancelled' });
          return;
        }
      }

      const tag = (document.activeElement?.tagName ?? '').toLowerCase();
      if (tag === 'input' || tag === 'textarea') return;
      if (e.key >= '1' && e.key <= '4') {
        const chosen = THEME_ORDER[parseInt(e.key, 10) - 1];
        if (chosen) activateTheme(chosen, true, view === 'landing');
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [pickerActive, pickerIndex, view, activateTheme, appendLog]);

  const heroCmd = useMemo(
    () => INSTALL_SNIPPETS_BY_REGION[installRegion][installTab].split('\n')[0] ?? '',
    [installRegion, installTab],
  );

  return (
    <div id="kiseki-app" data-ui-theme={layers.ui} data-typo-theme={layers.typography} style={appCssVars}>
      <div className="k-film-grain fixed inset-0 z-0" />

      <BootCurtain done={bootDone} />

      <Header
        view={view}
        scrolled={scrolled}
        lang={lang}
        onNavigate={(target) => navigateTo(target)}
        onToggleLang={() => setLang((l) => (l === 'zh' ? 'en' : 'zh'))}
      />

      <div id="k-view-landing" className={view === 'docs' ? 'hidden' : ''}>
        <HeroSection
          kvCanvasRef={kvCanvasRef}
          fxCanvasRef={fxCanvasRef}
          onParallaxMove={(x, y) => fxStageRef.current?.setMouse(x, y)}
          onParallaxLeave={() => fxStageRef.current?.setMouse(-9999, -9999)}
          prefersReducedMotion={prefersReducedMotion}
          wipeTick={wipeTick}
          atmosphereId={layers.atmosphere}
          typographyId={layers.typography}
          profile={profile}
          lang={lang}
          heroCmd={heroCmd}
          onSelectTheme={(id) => activateTheme(id, true, true)}
          onToggleShader={() => setProfile((p) => (p === 'static' ? 'balanced' : 'static'))}
          onNavigate={(target) => navigateTo(target)}
        />

        <LiveTerminal
          termCanvasRef={termCanvasRef}
          inputRef={termInputRef}
          prefersReducedMotion={prefersReducedMotion}
          activeTheme={activeTheme}
          layers={layers}
          profile={profile}
          txCounter={txCounter}
          narrowSplit={narrowSplit}
          lang={lang}
          pickerActive={pickerActive}
          pickerIndex={pickerIndex}
          logLines={logLines}
          onExecuteCmd={executeCmd}
          onSelectTheme={(id, withWipe = true) => activateTheme(id, true, withWipe)}
          onPickIndex={(idx) => {
            const chosen = THEME_ORDER[idx];
            setPickerActive(false);
            if (chosen) activateTheme(chosen, true, false);
          }}
          onToggleSplit={() => setNarrowSplit((s) => !s)}
          onCycleShader={() => {
            const order: ShaderProfile[] = ['static', 'eco', 'balanced', 'ultra'];
            setProfile((p) => order[(order.indexOf(p) + 1) % order.length]!);
          }}
        />

        <PromisesSection lang={lang} />

        <InstallSection
          lang={lang}
          atmosphereId={layers.atmosphere}
          detectedOS={detectedOS}
          installTab={installTab}
          installRegion={installRegion}
          onSelectInstallTab={setInstallTab}
          onSelectInstallRegion={setInstallRegion}
          onOpenDoc={(docId) => navigateTo('docs', docId)}
        />
      </div>

      {view === 'docs' && (
        <DocsHub
          lang={lang}
          installRegion={installRegion}
          docChapter={docChapter}
          docSearch={docSearch}
          onSelectChapter={(id) => {
            setDocChapter(id);
            window.history.replaceState(null, '', `#docs/${id}`);
          }}
          onSearchChange={setDocSearch}
          onNavigate={(target) => navigateTo(target)}
        />
      )}
    </div>
  );
};

export default App;
