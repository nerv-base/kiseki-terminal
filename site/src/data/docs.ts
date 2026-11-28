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
    keywords: 'install brew curl ps1 powershell init ghostty kitty wezterm iterm2 alacritty windows terminal bash fish zsh deps fonts',
  },
  {
    id: 'cli',
    num: '02',
    titleZh: '命令参考',
    titleEn: 'CLI Reference',
    keywords: 'init pick use compose profile live bg font fonts deps fetch status doctor rollback recover safe theme shell-init completions migrate uninstall',
  },
  {
    id: 'compose',
    num: '03',
    titleZh: '主题与 5 层混搭',
    titleEn: 'Themes & Compose',
    keywords: 'palette syntax atmosphere typography overrides prompt compose profile static eco balanced ultra accessibility',
  },
  {
    id: 'sdk',
    num: '04',
    titleZh: '制作社区主题包',
    titleEn: 'Theme SDK & Shaders',
    keywords: 'theme init manifest.json tokens.json live.glsl live.hlsl naga validate KISEKI_PARAM contributing',
  },
  {
    id: 'faq',
    num: '05',
    titleZh: '架构与常见问题',
    titleEn: 'Architecture & FAQ',
    keywords: 'nix home-manager render-only state.gen rollback recover safe migrate uninstall tier2 neovim helix tmux zellij bat btop',
  },
];

export const CLI_COMMANDS: Array<[string, string, string]> = [
  ['kiseki init', '交互式初始化向导（支持 --non-interactive / --render-only）', 'Interactive setup wizard (--non-interactive / --render-only)'],
  ['kiseki pick', '方向键交互式主题选择器（实时预览，Esc 零副作用撤回）', 'Interactive TUI picker with live preview & Esc cancel'],
  ['kiseki use <id>', '切换指定主题（支持 --only <adapters> 启用 Tier 2 生态工具）', 'Switch active theme (pass --only <adapters> for Tier 2 opt-in)'],
  ['kiseki compose', '按层混搭主题（--palette / --syntax / --atmosphere / --typography / --overrides / --prompt / --reset）', 'Mix layers (--palette / --syntax / --atmosphere / --typography / --overrides / --prompt / --reset)'],
  ['kiseki profile <tier>', '切换渲染档位：static / eco / balanced / ultra / accessibility', 'Switch shader tier: static / eco / balanced / ultra / accessibility'],
  ['kiseki live [on|off]', '快速开关动态 Shader（balanced ↔ static）', 'Toggle live shader (balanced ↔ static)'],
  ['kiseki bg <0..1|off|reset>', '调整壁纸透明度、关闭或恢复主题默认值', 'Set wallpaper opacity, disable, or reset'],
  ['kiseki font', '覆盖字体栈或字号，支持 --lock / --unlock 锁定不被主题覆盖', 'Override font stack/size (--lock / --unlock)'],
  ['kiseki status [--json]', '查看当前主题、混搭层、适配器、依赖摘要与产物状态', 'Show active theme, adapters, deps summary & artifacts (--json)'],
  ['kiseki deps [--json]', '查看外部工具来源（system / vendored / missing）、版本与修复指引', 'Inspect tool sources (system / vendored / missing), versions & remedies'],
  ['kiseki fonts status', '检查当前主题推荐字体的本机匹配与安装状态', 'Check local font availability and catalog resolution'],
  ['kiseki doctor', '全链路诊断：路径权限、字体、依赖、naga 校验与宿主配置验证', 'Full diagnostics: paths, fonts, deps, naga shaders & host configs'],
  ['kiseki rollback [tx-id]', '回滚到上一次或指定历史事务快照', 'Restore previous or specific atomic transaction snapshot'],
  ['kiseki recover --safe', '安全逃生：强制静态模式、关闭 Shader、修复损坏挂载', 'Safe escape: force static mode, disable shader, repair state'],
  ['kiseki gallery <subcmd>', 'search | info | refresh | sources 浏览社区市场（按热门/下载/Star/最新排序）', 'search | info | refresh | sources — browse Kiseki Gallery by trending/downloads/stars'],
  ['kiseki theme <subcmd>', 'init | list | info | validate | install <id|path|github:...> | update | remove', 'init | list | info | validate | install <id|path|github:...> | update | remove'],
  ['kiseki shell-init <shell>', '输出零 fork Shell 初始化脚本（zsh / bash / fish / pwsh）', 'Emit zero-fork shell sync hook (zsh / bash / fish / pwsh)'],
  ['kiseki completions <shell>', '生成 bash / zsh / fish / powershell / elvish 自动补全脚本', 'Generate completions for bash / zsh / fish / powershell / elvish'],
  ['kiseki fetch', '使用当前主题配置渲染 Fastfetch 系统信息卡', 'Render themed Fastfetch system banner'],
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
