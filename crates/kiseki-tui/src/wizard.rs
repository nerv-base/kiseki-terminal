//! Ultra-simple "One-Enter" interactive `kiseki init` wizard.
//!
//! Default path for normal users: pick a theme (`↑`/`↓`) and press `Enter`.
//! Advanced toggles stay tucked behind a `c` customize drawer so the initial
//! screen stays effortless and uncluttered. If an existing user Prompt config
//! (Oh My Posh / Starship) is detected, an explicit inline prompt-policy choice
//! is surfaced directly on the main screen (ADR-006).

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use kiseki_types::{KisekiError, PromptPolicy, RenderingProfile, Result};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::logo::{logo_ratatui_lines, prefer_ascii_logo, ThemeSwatch};
use crate::picker::TerminalGuard;
use crate::{InitWizardChoices, InitWizardInput, PickerItem};

/// Keyboard-independent wizard events for deterministic unit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardEvent {
    Up,
    Down,
    ToggleCustomize,
    CycleProfile,
    CyclePromptPolicy,
    ToggleAliases,
    ToggleFonts,
    ToggleRenderOnly,
    Confirm,
    Cancel,
}

/// Terminal outcome of [`InitWizardSession::handle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardOutcome {
    Continue,
    Confirm(InitWizardChoices),
    Cancel,
}

/// Rendering-agnostic state machine for the one-Enter `kiseki init` wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitWizardSession {
    pub input: InitWizardInput,
    pub cursor: usize,
    pub profile: RenderingProfile,
    pub prompt_policy: PromptPolicy,
    pub render_only: bool,
    pub aliases: bool,
    pub install_fonts: bool,
    pub customize_open: bool,
}

impl InitWizardSession {
    pub fn new(input: InitWizardInput) -> Self {
        let defaults = InitWizardChoices::non_interactive(&input);
        let cursor = input
            .themes
            .iter()
            .position(|item| item.id == defaults.theme)
            .unwrap_or(0);
        Self {
            // Interactive init defaults to full aesthetic experience (ADR-007):
            // prompt/enable curated font installation unless user toggles off.
            install_fonts: true,
            profile: defaults.profile,
            prompt_policy: defaults.prompt_policy,
            render_only: defaults.render_only,
            aliases: defaults.aliases,
            cursor,
            customize_open: false,
            input,
        }
    }

    pub fn current_theme(&self) -> Option<&PickerItem> {
        self.input.themes.get(self.cursor)
    }

    pub fn to_choices(&self) -> InitWizardChoices {
        let theme = self
            .current_theme()
            .map(|item| item.id.clone())
            .or_else(|| self.input.default_theme.clone())
            .unwrap_or_else(|| "nerv-eva".to_string());
        InitWizardChoices {
            theme,
            profile: self.profile,
            prompt_policy: self.prompt_policy,
            render_only: self.render_only,
            aliases: self.aliases,
            install_fonts: self.install_fonts,
        }
    }

    pub fn handle(&mut self, ev: WizardEvent) -> WizardOutcome {
        match ev {
            WizardEvent::Up => {
                if !self.input.themes.is_empty() {
                    self.cursor = if self.cursor == 0 {
                        self.input.themes.len() - 1
                    } else {
                        self.cursor - 1
                    };
                }
                WizardOutcome::Continue
            }
            WizardEvent::Down => {
                if !self.input.themes.is_empty() {
                    self.cursor = (self.cursor + 1) % self.input.themes.len();
                }
                WizardOutcome::Continue
            }
            WizardEvent::ToggleCustomize => {
                self.customize_open = !self.customize_open;
                WizardOutcome::Continue
            }
            WizardEvent::CycleProfile => {
                self.profile = next_profile(self.profile);
                WizardOutcome::Continue
            }
            WizardEvent::CyclePromptPolicy => {
                self.prompt_policy = next_prompt_policy(self.prompt_policy);
                WizardOutcome::Continue
            }
            WizardEvent::ToggleAliases => {
                self.aliases = !self.aliases;
                WizardOutcome::Continue
            }
            WizardEvent::ToggleFonts => {
                self.install_fonts = !self.install_fonts;
                WizardOutcome::Continue
            }
            WizardEvent::ToggleRenderOnly => {
                self.render_only = !self.render_only;
                WizardOutcome::Continue
            }
            WizardEvent::Confirm => WizardOutcome::Confirm(self.to_choices()),
            WizardEvent::Cancel => WizardOutcome::Cancel,
        }
    }
}

fn next_profile(current: RenderingProfile) -> RenderingProfile {
    match current {
        RenderingProfile::Balanced => RenderingProfile::Eco,
        RenderingProfile::Eco => RenderingProfile::Static,
        RenderingProfile::Static => RenderingProfile::Ultra,
        RenderingProfile::Ultra => RenderingProfile::Accessibility,
        RenderingProfile::Accessibility => RenderingProfile::Balanced,
    }
}

fn next_prompt_policy(current: PromptPolicy) -> PromptPolicy {
    match current {
        PromptPolicy::PaletteOnly => PromptPolicy::ManagedTemplate,
        PromptPolicy::ManagedTemplate => PromptPolicy::Disabled,
        PromptPolicy::Disabled => PromptPolicy::PaletteOnly,
    }
}

pub(crate) fn run(input: InitWizardInput) -> Result<Option<InitWizardChoices>> {
    let mut session = InitWizardSession::new(input);
    let mut guard = TerminalGuard::enter()?;
    let ascii_only = prefer_ascii_logo();

    loop {
        guard.draw(|frame| draw_wizard(frame, &session, ascii_only))?;

        let Event::Key(key) = event::read().map_err(|e| KisekiError::other(e.to_string()))? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(None);
        }

        let ev = match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(WizardEvent::Up),
            KeyCode::Down | KeyCode::Char('j') => Some(WizardEvent::Down),
            KeyCode::Enter => Some(WizardEvent::Confirm),
            KeyCode::Esc | KeyCode::Char('q') => Some(WizardEvent::Cancel),
            KeyCode::Char('c') => Some(WizardEvent::ToggleCustomize),
            KeyCode::Char('p') | KeyCode::Tab => Some(WizardEvent::CyclePromptPolicy),
            KeyCode::Char('m') => Some(WizardEvent::CycleProfile),
            KeyCode::Char('a') => Some(WizardEvent::ToggleAliases),
            KeyCode::Char('f') => Some(WizardEvent::ToggleFonts),
            KeyCode::Char('r') => Some(WizardEvent::ToggleRenderOnly),
            _ => None,
        };

        if let Some(ev) = ev {
            match session.handle(ev) {
                WizardOutcome::Continue => {}
                WizardOutcome::Confirm(choices) => return Ok(Some(choices)),
                WizardOutcome::Cancel => return Ok(None),
            }
        }
    }
}

fn draw_wizard(frame: &mut Frame<'_>, session: &InitWizardSession, ascii_only: bool) {
    let area = frame.area();
    let swatch = session
        .current_theme()
        .map(|item| item.swatch)
        .unwrap_or_default();

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(swatch.r_border()))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "KISEKI TERMINAL v2",
                Style::default()
                    .fg(swatch.r_accent())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · 一键初始化 ", Style::default().fg(swatch.r_fg())),
        ]))
        .title_bottom(
            Line::from(Span::styled(" kisekiterm.dev ", Style::default().fg(swatch.r_muted())))
                .alignment(Alignment::Right),
        );

    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.height < 8 || inner.width < 28 {
        let msg = Paragraph::new("按 Enter 立即应用默认配置，按 Esc 取消")
            .style(Style::default().fg(swatch.r_fg()));
        frame.render_widget(msg, inner);
        return;
    }

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(2)])
        .split(inner);

    let main_area = vertical[0];
    let footer_area = vertical[1];

    if main_area.width >= 72 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(38)])
            .split(main_area);
        draw_logo_sidebar(frame, cols[0], session, &swatch, ascii_only);
        draw_setup_column(frame, cols[1], session, &swatch);
    } else {
        draw_setup_column(frame, main_area, session, &swatch);
    }

    let hint = if session.customize_open {
        "↑/↓ 选主题 · Enter 立即应用 · m 档位 · p Prompt · f 字体 · a 别名 · r 只读 · c 收起"
    } else if session.input.user_prompt_config {
        "↑/↓ 选主题 · p 切换 Prompt 策略 · Enter 立即开始 · c 自定义选项 · Esc 取消"
    } else {
        "↑/↓ 选主题 · Enter 立即开始使用 · c 展开自定义选项 · Esc 取消"
    };
    let footer = Paragraph::new(Line::from(vec![
        Span::raw(" "),
        Span::styled(hint, Style::default().fg(swatch.r_muted())),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(swatch.r_surface())),
    );
    frame.render_widget(footer, footer_area);
}

fn draw_logo_sidebar(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &InitWizardSession,
    swatch: &ThemeSwatch,
    ascii_only: bool,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(12), Constraint::Min(3)])
        .split(area);

    let mut logo_lines = logo_ratatui_lines(swatch, ascii_only);
    for line in &mut logo_lines {
        *line = line.clone().alignment(Alignment::Center);
    }
    frame.render_widget(Paragraph::new(logo_lines), rows[0]);

    let mut info = Vec::new();
    if let Some(theme) = session.current_theme() {
        info.push(
            Line::from(Span::styled(
                theme.name.clone(),
                Style::default()
                    .fg(swatch.r_fg())
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
        );
        info.push(
            Line::from(vec![
                Span::styled("██", Style::default().fg(swatch.r_accent())),
                Span::raw(" "),
                Span::styled("██", Style::default().fg(swatch.r_secondary())),
                Span::raw(" "),
                Span::styled("██", Style::default().fg(swatch.r_tertiary())),
                Span::raw(" "),
                Span::styled("██", Style::default().fg(swatch.r_star())),
                Span::raw(" "),
                Span::styled("██", Style::default().fg(swatch.r_green())),
            ])
            .alignment(Alignment::Center),
        );
        if let Some(sub) = &theme.subtitle {
            info.push(Line::raw(""));
            info.push(
                Line::from(Span::styled(
                    sub.clone(),
                    Style::default().fg(swatch.r_muted()),
                ))
                .alignment(Alignment::Center),
            );
        }
    }
    frame.render_widget(Paragraph::new(info).wrap(Wrap { trim: true }), rows[1]);
}

fn draw_setup_column(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &InitWizardSession,
    swatch: &ThemeSwatch,
) {
    let env_height = if session.input.v1_detected { 4 } else { 3 };
    let prompt_height = if session.input.user_prompt_config && !session.customize_open {
        3
    } else {
        0
    };
    let custom_height = if session.customize_open { 7 } else { 0 };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(env_height),
            Constraint::Length(prompt_height),
            Constraint::Length(custom_height),
        ])
        .split(area);

    // 1. Theme selector (always focused — pick & Enter!)
    let items: Vec<ListItem<'static>> = session
        .input
        .themes
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let selected = idx == session.cursor;
            let pointer = if selected { "▸ " } else { "  " };
            let name_style = if selected {
                Style::default()
                    .fg(swatch.r_accent())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(swatch.r_fg())
            };
            ListItem::new(Line::from(vec![
                Span::styled(pointer, Style::default().fg(swatch.r_accent())),
                Span::styled(format!("{:<16}", item.id), name_style),
                Span::styled("█", Style::default().fg(item.swatch.r_accent())),
                Span::styled("█", Style::default().fg(item.swatch.r_secondary())),
                Span::styled("█", Style::default().fg(item.swatch.r_star())),
                Span::raw("  "),
                Span::styled(item.name.clone(), Style::default().fg(swatch.r_fg())),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(swatch.r_accent()))
                .title(Span::styled(
                    " 选择首发主题（按 Enter 直接生效） ",
                    Style::default()
                        .fg(swatch.r_fg())
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .highlight_style(Style::default().bg(swatch.r_surface()));
    let mut state = ListState::default();
    if !session.input.themes.is_empty() {
        state.select(Some(session.cursor));
    }
    frame.render_stateful_widget(list, sections[0], &mut state);

    // 2. Smart Defaults summary bar
    let terminal = session
        .input
        .detected_terminal
        .as_deref()
        .unwrap_or("自动探测");
    let shell = session
        .input
        .detected_shell
        .as_deref()
        .and_then(|s| s.rsplit('/').next())
        .unwrap_or("当前 Shell");
    let mode = if session.render_only {
        "render-only"
    } else {
        "非侵入自动挂载"
    };
    let mut summary_lines = vec![Line::from(vec![
        Span::styled("✓ ", Style::default().fg(swatch.r_green())),
        Span::styled(
            format!(
                "环境: {terminal} · {shell} · 档位 {} · {mode}",
                profile_label(session.profile)
            ),
            Style::default().fg(swatch.r_fg()),
        ),
    ])];
    if session.input.v1_detected {
        summary_lines.push(Line::from(vec![
            Span::styled("✦ ", Style::default().fg(swatch.r_star())),
            Span::styled(
                "检测到 v1 旧版配置 — 确认后将自动备份、清理旧钩子并迁移偏好",
                Style::default().fg(swatch.r_yellow()),
            ),
        ]));
    }
    let summary = Paragraph::new(summary_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(swatch.r_surface()))
                .title(Span::styled(" 智能默认 ", Style::default().fg(swatch.r_muted()))),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(summary, sections[1]);

    // 3. Conditional Prompt choice when user already has OMP/Starship (ADR-006)
    if sections[2].height > 0 {
        let prompt_bar = Paragraph::new(prompt_policy_line(session.prompt_policy, swatch)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(swatch.r_yellow()))
                .title(Span::styled(
                    " 检测到已有 Prompt 配置（按 p 切换） ",
                    Style::default().fg(swatch.r_yellow()),
                )),
        );
        frame.render_widget(prompt_bar, sections[2]);
    }

    // 4. Optional customize drawer (opened via `c`)
    if sections[3].height > 0 {
        let lines = vec![
            Line::from(vec![
                Span::styled(" [m] ", Style::default().fg(swatch.r_accent())),
                Span::styled("渲染档位: ", Style::default().fg(swatch.r_muted())),
                Span::styled(profile_label(session.profile), Style::default().fg(swatch.r_fg())),
            ]),
            Line::from(vec![
                Span::styled(" [p] ", Style::default().fg(swatch.r_accent())),
                Span::styled("Prompt : ", Style::default().fg(swatch.r_muted())),
                Span::styled(prompt_label(session.prompt_policy), Style::default().fg(swatch.r_fg())),
            ]),
            Line::from(vec![
                Span::styled(" [f] ", Style::default().fg(swatch.r_accent())),
                Span::styled("推荐字体: ", Style::default().fg(swatch.r_muted())),
                Span::styled(
                    if session.install_fonts { "缺失时提示安装" } else { "跳过，仅用本机字体" },
                    Style::default().fg(swatch.r_fg()),
                ),
            ]),
            Line::from(vec![
                Span::styled(" [a] ", Style::default().fg(swatch.r_accent())),
                Span::styled("短别名  : ", Style::default().fg(swatch.r_muted())),
                Span::styled(
                    if session.aliases { "开启 theme / themes" } else { "关闭（推荐，仅用 kiseki）" },
                    Style::default().fg(swatch.r_fg()),
                ),
            ]),
            Line::from(vec![
                Span::styled(" [r] ", Style::default().fg(swatch.r_accent())),
                Span::styled("写入模式: ", Style::default().fg(swatch.r_muted())),
                Span::styled(
                    if session.render_only { "render-only（不改宿主配置）" } else { "正常托管（自动兼容软链接）" },
                    Style::default().fg(swatch.r_fg()),
                ),
            ]),
        ];
        let drawer = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(swatch.r_secondary()))
                .title(Span::styled(
                    " 自定义选项（按 c 收起） ",
                    Style::default().fg(swatch.r_secondary()),
                )),
        );
        frame.render_widget(drawer, sections[3]);
    }
}

fn prompt_policy_line(policy: PromptPolicy, swatch: &ThemeSwatch) -> Line<'static> {
    let opt = |target: PromptPolicy, label: &'static str| {
        if policy == target {
            Span::styled(
                format!("● {label}  "),
                Style::default()
                    .fg(swatch.r_accent())
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!("○ {label}  "), Style::default().fg(swatch.r_muted()))
        }
    };
    Line::from(vec![
        Span::raw(" "),
        opt(PromptPolicy::PaletteOnly, "仅注入配色（保留原布局）"),
        opt(PromptPolicy::ManagedTemplate, "备份并启用官方模板"),
        opt(PromptPolicy::Disabled, "跳过"),
    ])
}

fn profile_label(profile: RenderingProfile) -> &'static str {
    match profile {
        RenderingProfile::Static => "Static（纯静态零 GPU）",
        RenderingProfile::Eco => "Eco（低功耗轻特效）",
        RenderingProfile::Balanced => "Balanced（推荐 60fps）",
        RenderingProfile::Ultra => "Ultra（全特效）",
        RenderingProfile::Accessibility => "Accessibility（高对比无动画）",
    }
}

fn prompt_label(policy: PromptPolicy) -> &'static str {
    match policy {
        PromptPolicy::PaletteOnly => "仅注入配色（保留原布局）",
        PromptPolicy::ManagedTemplate => "启用主题完整模板",
        PromptPolicy::Disabled => "跳过 Prompt 管理",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input(user_prompt_config: bool) -> InitWizardInput {
        InitWizardInput {
            themes: vec![
                PickerItem::new("nerv-eva", "NERV UNIT-02"),
                PickerItem::new("starlight-8bit", "Starlight 8-Bit"),
            ],
            detected_terminal: Some("ghostty".into()),
            detected_shell: Some("/bin/zsh".into()),
            v1_detected: true,
            user_prompt_config,
            default_render_only: false,
            default_theme: Some("nerv-eva".into()),
            default_profile: None,
        }
    }

    #[test]
    fn detected_default_profile_is_preserved() {
        let mut input = sample_input(false);
        input.default_profile = Some(RenderingProfile::Static);
        let session = InitWizardSession::new(input);
        assert_eq!(session.profile, RenderingProfile::Static);
    }

    #[test]
    fn one_enter_confirms_safe_defaults() {
        let mut session = InitWizardSession::new(sample_input(true));
        assert!(!session.customize_open);
        assert_eq!(session.prompt_policy, PromptPolicy::PaletteOnly);
        assert!(session.install_fonts);
        let outcome = session.handle(WizardEvent::Confirm);
        match outcome {
            WizardOutcome::Confirm(choices) => {
                assert_eq!(choices.theme, "nerv-eva");
                assert_eq!(choices.profile, RenderingProfile::Balanced);
                assert_eq!(choices.prompt_policy, PromptPolicy::PaletteOnly);
                assert!(!choices.aliases);
                assert!(choices.install_fonts);
            }
            other => panic!("expected Confirm, got {other:?}"),
        }
    }

    #[test]
    fn customize_toggles_and_navigation_work() {
        let mut session = InitWizardSession::new(sample_input(false));
        assert_eq!(session.prompt_policy, PromptPolicy::ManagedTemplate);
        session.handle(WizardEvent::Down);
        session.handle(WizardEvent::ToggleCustomize);
        assert!(session.customize_open);
        session.handle(WizardEvent::CycleProfile);
        session.handle(WizardEvent::CyclePromptPolicy);
        session.handle(WizardEvent::ToggleAliases);
        session.handle(WizardEvent::ToggleFonts);
        session.handle(WizardEvent::ToggleRenderOnly);

        let choices = session.to_choices();
        assert_eq!(choices.theme, "starlight-8bit");
        assert_eq!(choices.profile, RenderingProfile::Eco);
        assert_eq!(choices.prompt_policy, PromptPolicy::Disabled);
        assert!(choices.aliases);
        assert!(!choices.install_fonts);
        assert!(choices.render_only);
    }

    #[test]
    fn cancel_returns_cancel_without_mutating() {
        let mut session = InitWizardSession::new(sample_input(false));
        assert_eq!(session.handle(WizardEvent::Cancel), WizardOutcome::Cancel);
    }
}
