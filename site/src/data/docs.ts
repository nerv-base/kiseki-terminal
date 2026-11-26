import type { DocChapterId } from '../types/theme';

export interface DocChapterMeta {
  id: DocChapterId;
  num: string;
  titleZh: string;
  titleEn: string;
  keywords: string;
}

export const DOC_CHAPTERS_META: DocChapterMeta[] = [
  {
    id: 'quickstart',
    num: '01',
    titleZh: '快速开始',
    titleEn: 'Quickstart',
    keywords: 'install brew curl ps1 powershell init oh-my-posh fastfetch zsh-syntax-highlighting',
  },
  {
    id: 'cli',
    num: '02',
    titleZh: '命令参考',
    titleEn: 'CLI Reference',
    keywords: 'init pick use compose profile live bg font status doctor rollback recover safe theme migrate uninstall',
  },
  {
    id: 'compose',
    num: '03',
    titleZh: '主题与 5 层混搭',
    titleEn: 'Themes & Compose',
    keywords: 'palette syntax ui atmosphere typography compose profile static eco balanced ultra accessibility',
  },
  {
    id: 'sdk',
    num: '04',
    titleZh: '制作社区主题包',
    titleEn: 'Theme SDK & Shaders',
    keywords: 'manifest.json tokens.json live.glsl live.hlsl naga validate KISEKI_PARAM',
  },
  {
    id: 'faq',
    num: '05',
    titleZh: '架构与常见问题',
    titleEn: 'Architecture & FAQ',
    keywords: 'nix home-manager render-only state.gen rollback recover safe migrate uninstall',
  },
];

export const CLI_COMMANDS: Array<[string, string, string]> = [
  ['kiseki init', '交互式初始化向导（支持 --non-interactive / --render-only）', 'Interactive setup wizard (--non-interactive / --render-only)'],
  ['kiseki pick', '方向键交互式主题选择器（实时预览，Esc 零副作用撤回）', 'Interactive TUI picker with live preview & Esc cancel'],
  ['kiseki use <id>', '切换指定主题（starlight-8bit / kyoto-sakura / shinkai-rain / nerv-eva）', 'Switch active theme (starlight-8bit / kyoto-sakura / shinkai-rain / nerv-eva)'],
  ['kiseki compose', '按层混搭主题（--palette / --syntax / --ui / --atmosphere / --typography / --reset）', 'Mix theme layers (--palette / --atmosphere / --typography / --reset)'],
  ['kiseki profile <tier>', '切换渲染档位：static / eco / balanced / ultra / accessibility', 'Switch shader tier: static / eco / balanced / ultra / accessibility'],
  ['kiseki live [on|off]', '快速开关动态 Shader（balanced ↔ static）', 'Toggle live shader (balanced ↔ static)'],
  ['kiseki bg <0..1|off|reset>', '调整壁纸透明度、关闭或恢复主题默认值', 'Set wallpaper opacity, disable, or reset'],
  ['kiseki font', '覆盖字体栈或字号，支持 --lock / --unlock 锁定不被主题覆盖', 'Override font stack/size (--lock / --unlock)'],
  ['kiseki status [--json]', '查看当前主题、混搭层、适配器与生成产物状态', 'Show active theme, adapters & artifacts (--json)'],
  ['kiseki doctor', '全链路诊断：路径权限、字体、naga 校验与宿主配置验证', 'Full diagnostics: paths, fonts, naga shaders, host configs'],
  ['kiseki rollback [tx-id]', '回滚到上一次或指定历史事务快照', 'Restore previous or specific atomic transaction snapshot'],
  ['kiseki recover --safe', '安全逃生：强制静态模式、关闭 Shader、修复损坏挂载', 'Safe escape: force static mode, disable shader, repair state'],
  ['kiseki theme <subcmd>', 'list | info | validate | install | remove 管理与校验主题包', 'list | info | validate | install | remove theme packs'],
  ['kiseki migrate [--apply]', '探测并可逆迁移 Kiseki v1 旧版安装', 'Detect and reversibly migrate legacy v1 installs'],
  ['kiseki uninstall [--purge]', '干净撤销所有宿主挂载（加 --purge 清除本地数据）', 'Cleanly detach host mounts (--purge deletes local state)'],
];

export const COMPOSE_LAYERS: Array<{ layer: string; zh: string; en: string }> = [
  { layer: 'L1 palette', zh: '背景色阶、主辅强调色、语义色与 ANSI 16 色', en: 'Surfaces, accents, semantic colors & ANSI 16' },
  { layer: 'L2 syntax', zh: '19 项代码语义语法与 Zsh 命令高亮色板', en: '19 code highlight tokens & Zsh syntax highlighting' },
  { layer: 'L3 ui', zh: '标签栏、边框、状态栏与终端 Chrome 色', en: 'Tab bar, window frame & statusline tokens' },
  { layer: 'L4 atmosphere', zh: '二次元壁纸、Code Safe Zone 与 GLSL/HLSL 动态 Shader', en: 'Wallpaper, Code Safe Zone & GLSL/HLSL live shaders' },
  { layer: 'L5 typography', zh: '推荐字体栈、字号、行高与连字设置', en: 'Font stack, size, line-height & ligatures' },
];
