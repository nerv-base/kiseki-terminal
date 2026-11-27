import type { InstallRegion, InstallTab, Lang, ThemeId, WorldMeta } from '../types/theme';

export const THEME_ORDER: ThemeId[] = [
  'starlight-8bit',
  'kyoto-sakura',
  'shinkai-rain',
  'nerv-eva',
];

export const DEFAULT_OPENING_THEME: ThemeId = 'starlight-8bit';

export const WORLD_META: Record<ThemeId, WorldMeta> = {
  'starlight-8bit': {
    num: '01',
    shortZh: '星屑 8-Bit',
    shortEn: '8-Bit',
    hanko: '星屑',
    katakana: 'スターライト・八位元',
    typeTag: 'v2.0 // 8-BIT PIXEL',
    jpSubtitle: '奇蹟端末 · 星屑八位元',
    coords: 'ARCADE-88 // MIDNIGHT SKY',
    verticalJp: '「星屑の夜空、八位元の記憶。」',
    watermark: 'STARLIGHT',
    egg: '★ INSERT COIN // STARLIGHT READY',
  },
  'kyoto-sakura': {
    num: '02',
    shortZh: '夜桜',
    shortEn: 'Sakura',
    hanko: '夜桜',
    katakana: 'キセキ・ターミナル',
    typeTag: 'v2.0 // DIDONE · MINCHO',
    jpSubtitle: '奇蹟端末 · 京都夜桜',
    coords: "35°00'N 135°46'E // KYOTO SHRINE",
    verticalJp: '「月夜、端末に桜舞う。」',
    watermark: 'SAKURA',
    egg: '❀ 散華せず // KYOTO MOONLIGHT',
  },
  'shinkai-rain': {
    num: '03',
    shortZh: '言叶之雨',
    shortEn: 'Shinkai',
    hanko: '雨夜',
    katakana: 'キセキ・ターミナル',
    typeTag: 'v2.0 // CINEMA ULTRA-LIGHT',
    jpSubtitle: '奇蹟端末 · 言の葉の雨',
    coords: "35°41'N 139°45'E // TOKYO OVERPASS",
    verticalJp: '「雨音とコード、交差する夜。」',
    watermark: 'SHINKAI',
    egg: '◦ 言の葉の雨 // TOKYO 23:40',
  },
  'nerv-eva': {
    num: '04',
    shortZh: '弐号機',
    shortEn: 'UNIT-02',
    hanko: '弐号',
    katakana: '特務機関・奇蹟端末',
    typeTag: 'v2.0 // EVAMING · BARLOW',
    jpSubtitle: '特務機関 · 弐号機端末',
    coords: 'TOKYO-3 // GEOFRONT UNIT-02',
    verticalJp: '「沈黙の端末、再起動。」',
    watermark: 'UNIT-02',
    egg: 'MAGI-01 // SYNC RATE 99.2%',
  },
};

export const INSTALL_SNIPPETS_BY_REGION: Record<InstallRegion, Record<InstallTab, string>> = {
  cn: {
    brew: `brew install nerv-base/tap/kiseki\nkiseki init`,
    curl: `curl -fsSL https://kiseki-dl.nerv-base.com/install.sh | sh\nkiseki init`,
    ps1: `irm https://kiseki-dl.nerv-base.com/install.ps1 | iex\nkiseki init`,
  },
  global: {
    brew: `brew install nerv-base/tap/kiseki\nkiseki init`,
    curl: `curl -fsSL https://dl.kisekiterm.dev/install.sh | sh\nkiseki init`,
    ps1: `irm https://dl.kisekiterm.dev/install.ps1 | iex\nkiseki init`,
  },
};

export const INSTALL_SNIPPETS: Record<InstallTab, string> = INSTALL_SNIPPETS_BY_REGION.global;

export function detectInstallRegion(): InstallRegion {
  if (typeof window === 'undefined') return 'cn';
  try {
    const tz = Intl.DateTimeFormat().resolvedOptions().timeZone || '';
    if (/^(Asia\/(Shanghai|Chongqing|Harbin|Urumqi|Hong_Kong|Macau)|PRC)$/i.test(tz)) {
      return 'cn';
    }
    const lang = (navigator.language || '').toLowerCase();
    if (lang === 'zh-cn' || lang === 'zh-hans' || lang === 'zh') {
      return 'cn';
    }
  } catch {
    // Ignore Intl errors in restricted environments
  }
  return 'global';
}

export type DetectedOS = 'macos' | 'windows' | 'linux';

export function detectOS(): DetectedOS {
  if (typeof navigator === 'undefined') return 'macos';
  const ua = (navigator.userAgent || '').toLowerCase();
  if (ua.includes('win')) return 'windows';
  if (ua.includes('linux')) return 'linux';
  return 'macos';
}

export function defaultInstallTabForOS(os: DetectedOS): InstallTab {
  if (os === 'windows') return 'ps1';
  if (os === 'linux') return 'curl';
  return 'brew';
}

export const I18N: Record<
  Lang,
  {
    nav_home: string;
    nav_try: string;
    nav_dl: string;
    nav_docs: string;
    hero_sub: string;
    copy: string;
    copied: string;
    cta_dl: string;
    cta_docs: string;
    cta_try: string;
    world_hint: string;
    act1_title: string;
    act1_desc: string;
    try_mix1: string;
    try_mix2: string;
    split_wide: string;
    split_narrow: string;
    act2_title: string;
    col1_t: string;
    col2_t: string;
    col3_t: string;
    m_start: string;
    m_hook: string;
    m_bin: string;
    act3_title: string;
    dl_cli_hdr: string;
    dl_init_tip_pre: string;
    dl_init_tip_post: string;
    dl_docs_link: string;
    region_cn: string;
    region_global: string;
    docs_title: string;
    docs_search_placeholder: string;
    back_home: string;
    go_dl: string;
  }
> = {
  zh: {
    nav_home: '首页',
    nav_try: '试玩',
    nav_dl: '下载',
    nav_docs: '文档',
    hero_sub: '给终端换上二次元壁纸、动态光影与配色。',
    copy: 'COPY',
    copied: 'COPIED ✓',
    cta_dl: '立即安装',
    cta_docs: '阅读文档 →',
    cta_try: '在线试玩 ↓',
    world_hint: 'SCENE SELECT // 按键 1–4 切换主题',
    act1_title: '「 在线试玩终端 」',
    act1_desc: '不用安装，直接点下面切换或敲命令看效果。',
    try_mix1: '混搭: 夜樱×星屑',
    try_mix2: '混搭: 弐号機×雨夜',
    split_wide: '视图: 宽屏 (Wide)',
    split_narrow: '视图: 窄分屏 (Narrow)',
    act2_title: '「 好用、安全、不折腾 」',
    col1_t: '不弄脏配置',
    col2_t: '自由混搭',
    col3_t: '开箱即用',
    m_start: '启动',
    m_hook: 'Hook',
    m_bin: '单文件',
    act3_title: '「 下载 & 安装 」',
    dl_cli_hdr: '命令行一键安装',
    dl_init_tip_pre: '安装后运行 ',
    dl_init_tip_post: ' 完成向导挂载',
    dl_docs_link: '查看完整安装文档 →',
    region_cn: '国内加速 · EdgeOne',
    region_global: 'Global · R2',
    docs_title: '奇蹟端末 使用文档',
    docs_search_placeholder: '搜索文档或命令...',
    back_home: '← 返回主页',
    go_dl: '前往下载页',
  },
  en: {
    nav_home: 'Home',
    nav_try: 'Try Live',
    nav_dl: 'Download',
    nav_docs: 'Docs',
    hero_sub: 'Live anime wallpapers, shaders & themes for your terminal.',
    copy: 'COPY',
    copied: 'COPIED ✓',
    cta_dl: 'Install Now',
    cta_docs: 'Read Docs →',
    cta_try: 'Try Live ↓',
    world_hint: 'SCENE SELECT // PRESS 1–4',
    act1_title: '「 Try It Live 」',
    act1_desc: 'No install required — click below or type a command.',
    try_mix1: 'Mix: Sakura × 8-Bit',
    try_mix2: 'Mix: UNIT-02 × Rain',
    split_wide: 'View: Wide Pane',
    split_narrow: 'View: Narrow Split',
    act2_title: '「 Simple. Safe. Native Rust. 」',
    col1_t: 'Clean Mounts',
    col2_t: 'Mix Layers',
    col3_t: 'Ready Out of Box',
    m_start: 'Startup',
    m_hook: 'Hook',
    m_bin: 'Binary',
    act3_title: '「 Download & Install 」',
    dl_cli_hdr: 'One-Line CLI Install',
    dl_init_tip_pre: 'Run ',
    dl_init_tip_post: ' after installing',
    dl_docs_link: 'Full Installation Guide →',
    region_cn: 'China CDN · EdgeOne',
    region_global: 'Global · R2',
    docs_title: 'Kiseki Documentation',
    docs_search_placeholder: 'Search docs or commands...',
    back_home: '← Back to Home',
    go_dl: 'Go to Download',
  },
};
