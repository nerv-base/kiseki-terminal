export type ThemeId = 'starlight-8bit' | 'kyoto-sakura' | 'shinkai-rain' | 'nerv-eva';

export type ShaderProfile = 'static' | 'eco' | 'balanced' | 'ultra';

export type Lang = 'zh' | 'en';

export type InstallTab = 'brew' | 'curl' | 'ps1';

export type InstallRegion = 'cn' | 'global';

export type ViewMode = 'landing' | 'docs';

export type DocChapterId = 'quickstart' | 'cli' | 'compose' | 'sdk' | 'faq';

export interface LayerState {
  palette: ThemeId;
  syntax: ThemeId;
  ui: ThemeId;
  atmosphere: ThemeId;
  typography: ThemeId;
}

export interface HistorySnapshot {
  activeTheme: ThemeId;
  layers: LayerState;
  profile: ShaderProfile;
  bgOpacityOverride: number | null;
}

export interface ThemeTokens {
  palette: {
    surfaces: {
      crust: string;
      mantle: string;
      base: string;
      surface0: string;
      surface1: string;
      surface2: string;
      overlay0: string;
      overlay1: string;
    };
    text: {
      primary: string;
      secondary: string;
      muted: string;
      inverse: string;
    };
    accents: {
      primary: string;
      secondary: string;
      tertiary: string;
    };
    semantic: {
      error: string;
      warning: string;
      success: string;
      info: string;
      hint: string;
    };
    ansi: {
      black: string;
      red: string;
      green: string;
      yellow: string;
      blue: string;
      magenta: string;
      cyan: string;
      white: string;
    };
  };
  syntax: Record<string, string>;
  ui: {
    tabBarBg: string;
    tabActiveBg: string;
    border: string;
    panelBg: string;
  };
  atmosphere: {
    window: {
      opacity?: number;
    };
    wallpaper: {
      opacity?: number;
      codeSafeZone?: {
        side?: string;
        startX?: number;
        endX?: number;
      };
    };
    shader: {
      defaultProfile?: ShaderProfile;
      params?: Record<string, number>;
    };
  };
  overrides: {
    prompt: {
      preset: string;
      badgeText: string;
      promptSymbol: string;
      consoleTitlePrefix: string;
      segmentColors: {
        badgeBg: string;
        badgeFg: string;
        pathBg: string;
        pathFg: string;
      };
    };
    fastfetch: {
      headerTitle: string;
      fieldLabels: Record<string, string>;
    };
  };
}

export interface ThemePack {
  manifest: {
    id: ThemeId;
    name: string;
    subtitle: string;
    version: string;
  };
  tokens: ThemeTokens;
  shaderGlsl: string;
  logoHtml: string;
  wallpaperUrl: string;
  thumbUrl: string;
}

export interface ThemesBundle {
  themes: Record<ThemeId, ThemePack>;
}

export interface WorldMeta {
  num: string;
  shortZh: string;
  shortEn: string;
  hanko: string;
  katakana: string;
  typeTag: string;
  jpSubtitle: string;
  coords: string;
  verticalJp: string;
  watermark: string;
  egg: string;
}
