//! Interactive Ratatui theme picker (`kiseki pick`).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use kiseki_types::{Appearance, KisekiError, Result};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::logo::{logo_ratatui_lines, prefer_ascii_logo, ThemeSwatch};
use crate::{PickerAction, PickerInput, PickerSession};

static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK_INSTALLED: Once = Once::new();

fn restore_terminal_best_effort() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen, Show);
}

fn install_panic_hook() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Works even under release `panic = "abort"`, where `Drop` does not run.
            if TUI_ACTIVE.swap(false, Ordering::SeqCst) {
                restore_terminal_best_effort();
            }
            prev(info);
        }));
    });
}

/// RAII terminal guard guaranteeing alternate screen & raw mode are restored,
/// including on panic under `panic = "abort"` release builds.
pub(crate) struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        install_panic_hook();
        enable_raw_mode().map_err(|e| KisekiError::other(e.to_string()))?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(KisekiError::other(err.to_string()));
        }
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = match Terminal::new(backend) {
            Ok(term) => term,
            Err(err) => {
                restore_terminal_best_effort();
                return Err(KisekiError::other(err.to_string()));
            }
        };
        TUI_ACTIVE.store(true, Ordering::SeqCst);
        let _ = terminal.hide_cursor();
        let _ = terminal.clear();
        Ok(Self { terminal })
    }

    pub fn draw<F>(&mut self, render: F) -> Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        self.terminal
            .draw(render)
            .map_err(|e| KisekiError::other(e.to_string()))?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        TUI_ACTIVE.store(false, Ordering::SeqCst);
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

pub(crate) fn run(session: &mut PickerSession) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let ascii_only = prefer_ascii_logo();
    let mut search_mode = false;
    let mut previewed: Option<String> = None;

    loop {
        guard.draw(|frame| draw_picker(frame, session, search_mode, previewed.as_deref(), ascii_only))?;

        let Event::Key(key) = event::read().map_err(|e| KisekiError::other(e.to_string()))? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            session.cancel();
            return Ok(());
        }

        let input = if search_mode {
            match key.code {
                KeyCode::Esc => {
                    if session.query().is_empty() {
                        search_mode = false;
                        None
                    } else {
                        while !session.query().is_empty() {
                            session.handle(PickerInput::Backspace);
                        }
                        search_mode = false;
                        None
                    }
                }
                KeyCode::Enter => {
                    search_mode = false;
                    // Focus first match if cursor isn't visible.
                    if !session.visible_indices().contains(&session.cursor()) {
                        session.handle(PickerInput::Home);
                    }
                    None
                }
                KeyCode::Up => Some(PickerInput::Up),
                KeyCode::Down => Some(PickerInput::Down),
                KeyCode::Backspace => Some(PickerInput::Backspace),
                KeyCode::Char(ch) => Some(PickerInput::Char(ch)),
                _ => None,
            }
        } else {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => Some(PickerInput::Up),
                KeyCode::Down | KeyCode::Char('j') => Some(PickerInput::Down),
                KeyCode::Home | KeyCode::Char('g') => Some(PickerInput::Home),
                KeyCode::End | KeyCode::Char('G') => Some(PickerInput::End),
                KeyCode::Enter => Some(PickerInput::Enter),
                KeyCode::Char(' ') => Some(PickerInput::Space),
                KeyCode::Esc | KeyCode::Char('q') => Some(PickerInput::Cancel),
                KeyCode::Char('/') => {
                    search_mode = true;
                    None
                }
                KeyCode::Backspace if !session.query().is_empty() => Some(PickerInput::Backspace),
                _ => None,
            }
        };

        if let Some(input) = input {
            let action = session.handle(input);
            match action {
                PickerAction::Select(_) | PickerAction::Cancel => return Ok(()),
                PickerAction::Preview(id) => previewed = Some(id),
                PickerAction::None => {}
            }
        }
    }
}

fn draw_picker(
    frame: &mut Frame<'_>,
    session: &PickerSession,
    search_mode: bool,
    previewed: Option<&str>,
    ascii_only: bool,
) {
    let area = frame.area();
    let swatch = session
        .current()
        .map(|item| item.swatch)
        .unwrap_or_default();

    // Outer container with subtle theme-tinted border.
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(swatch.r_border()))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "KISEKI",
                Style::default()
                    .fg(swatch.r_accent())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · 主题选择器 ", Style::default().fg(swatch.r_fg())),
        ]))
        .title_bottom(Line::from(vec![
            Span::styled(" kisekiterm.dev ", Style::default().fg(swatch.r_muted())),
        ]).alignment(Alignment::Right));

    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.height < 6 || inner.width < 24 {
        let fallback = Paragraph::new("窗口过小，请拉宽终端后重试（Esc 退出）")
            .style(Style::default().fg(swatch.r_fg()));
        frame.render_widget(fallback, inner);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(2)])
        .split(inner);

    let main_area = chunks[0];
    let footer_area = chunks[1];

    if main_area.width >= 68 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(32)])
            .split(main_area);
        draw_brand_panel(frame, cols[0], session, &swatch, ascii_only);
        draw_list_panel(frame, cols[1], session, search_mode, previewed, &swatch);
    } else {
        draw_list_panel(frame, main_area, session, search_mode, previewed, &swatch);
    }

    draw_footer(frame, footer_area, search_mode, &swatch);
}

fn draw_brand_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &PickerSession,
    swatch: &ThemeSwatch,
    ascii_only: bool,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12),
            Constraint::Min(3),
        ])
        .split(area);

    let mut logo_lines = logo_ratatui_lines(swatch, ascii_only);
    for line in &mut logo_lines {
        *line = line.clone().alignment(Alignment::Center);
    }
    let logo_widget = Paragraph::new(logo_lines);
    frame.render_widget(logo_widget, rows[0]);

    let mut meta_lines = Vec::new();
    if let Some(item) = session.current() {
        let appearance_tag = match item.appearance {
            Appearance::Dark => "DARK",
            Appearance::Light => "LIGHT",
        };
        meta_lines.push(
            Line::from(vec![
                Span::styled(
                    item.name.clone(),
                    Style::default()
                        .fg(swatch.r_fg())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("[{appearance_tag}]"),
                    Style::default().fg(swatch.r_muted()),
                ),
            ])
            .alignment(Alignment::Center),
        );
        // Live palette swatch strip
        meta_lines.push(
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
        if let Some(sub) = &item.subtitle {
            meta_lines.push(Line::raw(""));
            meta_lines.push(
                Line::from(Span::styled(
                    sub.clone(),
                    Style::default().fg(swatch.r_muted()),
                ))
                .alignment(Alignment::Center),
            );
        }
    }
    let meta = Paragraph::new(meta_lines).wrap(Wrap { trim: true });
    frame.render_widget(meta, rows[1]);
}

fn draw_list_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &PickerSession,
    search_mode: bool,
    previewed: Option<&str>,
    swatch: &ThemeSwatch,
) {
    let constraints = if search_mode || !session.query().is_empty() {
        vec![Constraint::Length(3), Constraint::Min(3)]
    } else {
        vec![Constraint::Length(0), Constraint::Min(3)]
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    if rows[0].height > 0 {
        let search_border = if search_mode {
            swatch.r_accent()
        } else {
            swatch.r_muted()
        };
        let search_text = if search_mode {
            format!("/ {}_", session.query())
        } else {
            format!("/ {}", session.query())
        };
        let search = Paragraph::new(search_text)
            .style(Style::default().fg(swatch.r_fg()))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(search_border))
                    .title(" 搜索主题 "),
            );
        frame.render_widget(search, rows[0]);
    }

    let visible = session.visible_indices();
    let mut selected_row = None;
    let list_items: Vec<ListItem<'static>> = visible
        .iter()
        .enumerate()
        .map(|(row_idx, &item_idx)| {
            let item = &session.items()[item_idx];
            let is_cur = item_idx == session.cursor();
            if is_cur {
                selected_row = Some(row_idx);
            }
            let pointer = if is_cur { "▸ " } else { "  " };
            let active_dot = if item.active { "● " } else { "○ " };
            let dot_style = if item.active {
                Style::default().fg(swatch.r_green())
            } else {
                Style::default().fg(swatch.r_muted())
            };
            let title_style = if is_cur {
                Style::default()
                    .fg(swatch.r_accent())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(swatch.r_fg())
            };
            let badge = item
                .badge
                .as_deref()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();

            let mut line1 = vec![
                Span::styled(pointer, Style::default().fg(swatch.r_accent())),
                Span::styled(active_dot, dot_style),
                Span::styled(format!("{:<18}", item.id), title_style),
                Span::styled("█", Style::default().fg(item.swatch.r_accent())),
                Span::styled("█", Style::default().fg(item.swatch.r_secondary())),
                Span::styled("█", Style::default().fg(item.swatch.r_star())),
                Span::raw("  "),
                Span::styled(item.name.clone(), Style::default().fg(swatch.r_fg())),
                Span::styled(badge, Style::default().fg(swatch.r_muted())),
            ];
            if previewed == Some(item.id.as_str()) && !item.active {
                line1.push(Span::styled("  (预览)", Style::default().fg(swatch.r_secondary())));
            }
            ListItem::new(Line::from(line1))
        })
        .collect();

    let count_title = format!(" 可用主题 ({}/{}) ", visible.len(), session.items().len());
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(swatch.r_border()))
        .title(Span::styled(count_title, Style::default().fg(swatch.r_fg())));

    if list_items.is_empty() {
        let empty = Paragraph::new("未找到匹配的主题，按 Esc 清除搜索词")
            .style(Style::default().fg(swatch.r_muted()))
            .block(list_block);
        frame.render_widget(empty, rows[1]);
        return;
    }

    let list = List::new(list_items)
        .block(list_block)
        .highlight_style(Style::default().bg(swatch.r_surface()));
    let mut state = ListState::default();
    state.select(selected_row);
    frame.render_stateful_widget(list, rows[1], &mut state);
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, search_mode: bool, swatch: &ThemeSwatch) {
    let hint = if search_mode {
        "输入关键词筛选 · ↑/↓ 移动 · Enter 确认筛选 · Esc 退出搜索"
    } else {
        "↑/↓ (j/k) 选择 · Enter 切换主题 · / 搜索 · Space 标记预览 · q/Esc 取消"
    };
    let line = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(hint, Style::default().fg(swatch.r_muted())),
    ]);
    let bar = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(swatch.r_surface())),
    );
    frame.render_widget(bar, area);
}
