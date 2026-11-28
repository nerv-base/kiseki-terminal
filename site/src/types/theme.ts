export type ThemeId = 'starlight-8bit' | 'kyoto-sakura' | 'shinkai-rain' | 'nerv-eva';

export type ShaderProfile = 'static' | 'eco' | 'balanced' | 'ultra';

export type Lang = 'zh' | 'en';

export type InstallTab = 'brew' | 'curl' | 'ps1';

export type ViewMode = 'landing' | 'gallery' | 'docs';

export type GallerySortKey = 'trending' | 'downloads' | 'stars' | 'updated' | 'newest';

export interface GalleryThemeEntry {
  id: string;
  name: string;
  subtitle: string;
  version: string;
  appearance: 'dark' | 'light';
  tags: string[];
  authors: Array<{ name: string; url?: string }>;
  license: string;
  repo: string;
  subpath?: string;
  commit?: string;
  createdAt: string;
  updatedAt: string;
  builtin?: boolean;
  capabilities: {
    hasWallpaper: boolean;
    hasShader: boolean;
    shaderProfiles: string[];
    wcagGrade: string;
  };
  metrics: {
    stars: number;
    stars7d: number;
    downloads: number;
    downloads30d: number;
    trendingScore: number;
  };
  download?: {
    trackUrl?: string | null;
    url: string;
    sha256: string;
    bytes: number;
  };
  preview: {
    thumbUrl?: string | null;
    /** Optional wallpaper asset URL used by built-in theme live terminal previews. */
    wallpaperUrl?: string | null;
    /** Optional opacity tuning (0..1) for realistic wallpaper blending inside previews. */
    wallpaperOpacity?: number;
    /** Optional prompt chrome preview colors. */
    prompt?: {
      badgeText: string;
      badgeBg: string;
      badgeFg: string;
      pathBg: string;
      pathFg: string;
      symbol: string;
    };
    palette: {
      base: string;
      surface0: string;
      textPrimary: string;
      accentPrimary: string;
      accentSecondary: string;
      keyword: string;
      string: string;
      function: string;
    };
  };
}

export interface GalleryIndexPayload {
  schemaVersion: number;
  generatedAt: string;
  themes: GalleryThemeEntry[];
}

export interface GalleryUpdatesPayload {
  schemaVersion: number;
  themes: Record<
    string,
    {
      latestTag?: string | null;
      releaseNotes?: string | null;
      recentCommits?: string[];
    }
  >;
}

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
