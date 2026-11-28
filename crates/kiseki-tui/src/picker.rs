//! Interactive Ratatui theme picker (`kiseki pick`) with lazy-loaded Gallery tab.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Once;
use std::time::Duration;

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
use crate::{
    GalleryLoadState, GalleryPickerItem, PickerAction, PickerInput, PickerSession, PickerTab,
};

pub(crate) type GalleryLoader = Box<dyn FnOnce() -> Result<Vec<GalleryPickerItem>> + Send + 'static>;

static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK_INSTALLED: Once = Once::new();
const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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

pub(crate) fn run(session: &mut PickerSession, mut loader: Option<GalleryLoader>) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let ascii_only = prefer_ascii_logo();
    let mut search_mode = false;
    let mut previewed: Option<String> = None;
    let mut gallery_rx: Option<Receiver<Result<Vec<GalleryPickerItem>>>> = None;
    let mut tick: usize = 0;

    loop {
        if let Some(rx) = gallery_rx.as_ref() {
            match rx.try_recv() {
                Ok(Ok(items)) => {
                    session.set_gallery_state(GalleryLoadState::Ready(items));
                    gallery_rx = None;
                }
                Ok(Err(error)) => {
                    session.set_gallery_state(GalleryLoadState::Failed(error.to_string()));
                    gallery_rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    session.set_gallery_state(GalleryLoadState::Failed(
                        "gallery loader exited unexpectedly".to_string(),
                    ));
                    gallery_rx = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        guard.draw(|frame| {
            draw_picker(
                frame,
                session,
                search_mode,
                previewed.as_deref(),
                ascii_only,
                tick,
            )
        })?;

        let poll_ms = if gallery_rx.is_some() { 80 } else { 250 };
        if !event::poll(Duration::from_millis(poll_ms))
            .map_err(|e| KisekiError::other(e.to_string()))?
        {
            tick = tick.wrapping_add(1);
            continue;
        }

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
                    let cursor_visible = match session.tab() {
                        PickerTab::Local => session.visible_indices().contains(&session.cursor()),
                        PickerTab::Gallery => session
                            .visible_gallery_indices()
                            .contains(&session.gallery_cursor()),
                    };
                    if !cursor_visible {
                        session.handle(PickerInput::Home);
                    }
                    None
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    search_mode = false;
                    Some(PickerInput::SwitchTab)
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
                KeyCode::Tab | KeyCode::BackTab => Some(PickerInput::SwitchTab),
                KeyCode::Char('s') if session.tab() == PickerTab::Gallery => {
                    Some(PickerInput::CycleSort)
                }
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
            // Lazily spawn the gallery index load the first time the user enters Gallery.
            if session.tab() == PickerTab::Gallery
                && matches!(session.gallery_state(), GalleryLoadState::Idle)
            {
                if let Some(load_fn) = loader.take() {
                    session.set_gallery_state(GalleryLoadState::Loading);
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(load_fn());
                    });
                    gallery_rx = Some(rx);
                }
            }
            match action {
                PickerAction::Select(_)
                | PickerAction::InstallGallery(_)
                | PickerAction::Cancel => return Ok(()),
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
    tick: usize,
) {
    let area = frame.area();
    let swatch = session.active_swatch();

    let mut title_spans = vec![
        Span::raw(" "),
        Span::styled(
            "KISEKI",
            Style::default()
                .fg(swatch.r_accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(swatch.r_muted())),
    ];
    if session.gallery_enabled() {
        let local_style = if session.tab() == PickerTab::Local {
            Style::default()
                .fg(swatch.r_accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(swatch.r_muted())
        };
        let gallery_style = if session.tab() == PickerTab::Gallery {
            Style::default()
                .fg(swatch.r_accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(swatch.r_muted())
        };
        title_spans.push(Span::styled(
            format!("本地主题 ({})", session.items().len()),
            local_style,
        ));
        title_spans.push(Span::styled("  │  ", Style::default().fg(swatch.r_muted())));
        let gallery_label = match session.gallery_state() {
            GalleryLoadState::Ready(list) => format!("✦ 社区市场 ({})", list.len()),
            GalleryLoadState::Loading => "✦ 社区市场 (加载中...)".to_string(),
            _ => "✦ 社区市场 [Tab]".to_string(),
        };
        title_spans.push(Span::styled(gallery_label, gallery_style));
        title_spans.push(Span::raw(" "));
    } else {
        title_spans.push(Span::styled("主题选择器 ", Style::default().fg(swatch.r_fg())));
    }

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(swatch.r_border()))
        .title(Line::from(title_spans))
        .title_bottom(
            Line::from(vec![Span::styled(
                " kisekiterm.dev ",
                Style::default().fg(swatch.r_muted()),
            )])
            .alignment(Alignment::Right),
        );

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
        match session.tab() {
            PickerTab::Local => {
                draw_local_list_panel(frame, cols[1], session, search_mode, previewed, &swatch)
            }
            PickerTab::Gallery => {
                draw_gallery_list_panel(frame, cols[1], session, search_mode, &swatch, tick)
            }
        }
    } else {
        match session.tab() {
            PickerTab::Local => {
                draw_local_list_panel(frame, main_area, session, search_mode, previewed, &swatch)
            }
            PickerTab::Gallery => {
                draw_gallery_list_panel(frame, main_area, session, search_mode, &swatch, tick)
            }
        }
    }

    draw_footer(frame, footer_area, session, search_mode, &swatch);
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
        .constraints([Constraint::Length(12), Constraint::Min(3)])
        .split(area);

    let mut logo_lines = logo_ratatui_lines(swatch, ascii_only);
    for line in &mut logo_lines {
        *line = line.clone().alignment(Alignment::Center);
    }
    frame.render_widget(Paragraph::new(logo_lines), rows[0]);

    let mut meta_lines = Vec::new();
    match session.tab() {
        PickerTab::Local => {
            if let Some(item) = session.current() {
                push_swatch_header(&mut meta_lines, &item.name, item.appearance, swatch);
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
        }
        PickerTab::Gallery => {
            if let Some(item) = session.current_gallery() {
                push_swatch_header(&mut meta_lines, &item.name, item.appearance, swatch);
                meta_lines.push(
                    Line::from(Span::styled(
                        format!("★ {}  ↓ {}  v{}", item.stars, item.downloads, item.version),
                        Style::default().fg(swatch.r_star()),
                    ))
                    .alignment(Alignment::Center),
                );
                let mut caps = Vec::new();
                if item.has_wallpaper {
                    caps.push("壁纸");
                }
                if item.has_shader {
                    caps.push("Shader");
                }
                if !item.wcag_grade.is_empty() {
                    caps.push(item.wcag_grade.as_str());
                }
                if !caps.is_empty() {
                    meta_lines.push(
                        Line::from(Span::styled(
                            caps.join(" · "),
                            Style::default().fg(swatch.r_secondary()),
                        ))
                        .alignment(Alignment::Center),
                    );
                }
                if !item.subtitle.is_empty() {
                    meta_lines.push(Line::raw(""));
                    meta_lines.push(
                        Line::from(Span::styled(
                            item.subtitle.clone(),
                            Style::default().fg(swatch.r_muted()),
                        ))
                        .alignment(Alignment::Center),
                    );
                }
                if !item.author.is_empty() {
                    meta_lines.push(
                        Line::from(Span::styled(
                            format!("by {}", item.author),
                            Style::default().fg(swatch.r_muted()),
                        ))
                        .alignment(Alignment::Center),
                    );
                }
            }
        }
    }

    let meta = Paragraph::new(meta_lines).wrap(Wrap { trim: true });
    frame.render_widget(meta, rows[1]);
}

fn push_swatch_header(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    appearance: Appearance,
    swatch: &ThemeSwatch,
) {
    let appearance_tag = match appearance {
        Appearance::Dark => "DARK",
        Appearance::Light => "LIGHT",
    };
    lines.push(
        Line::from(vec![
            Span::styled(
                name.to_string(),
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
    lines.push(
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
}

fn draw_search_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    query: &str,
    search_mode: bool,
    swatch: &ThemeSwatch,
) {
    let search_border = if search_mode {
        swatch.r_accent()
    } else {
        swatch.r_muted()
    };
    let search_text = if search_mode {
        format!("/ {query}_")
    } else {
        format!("/ {query}")
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
    frame.render_widget(search, area);
}

fn draw_local_list_panel(
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
        draw_search_bar(frame, rows[0], session.query(), search_mode, swatch);
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
                line1.push(Span::styled(
                    "  (预览)",
                    Style::default().fg(swatch.r_secondary()),
                ));
            }
            ListItem::new(Line::from(line1))
        })
        .collect();

    let count_title = format!(" 本地主题 ({}/{}) ", visible.len(), session.items().len());
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

fn draw_gallery_list_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &PickerSession,
    search_mode: bool,
    swatch: &ThemeSwatch,
    tick: usize,
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
        draw_search_bar(frame, rows[0], session.query(), search_mode, swatch);
    }

    let sort_label = session.gallery_sort().label();
    let list_block = |title: String| {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(swatch.r_border()))
            .title(Span::styled(title, Style::default().fg(swatch.r_fg())))
    };

    match session.gallery_state() {
        GalleryLoadState::Idle | GalleryLoadState::Loading => {
            let spinner = SPINNER[tick % SPINNER.len()];
            let msg = Paragraph::new(format!(
                "\n  {spinner} 正在从 Kiseki Gallery 加载社区主题索引...\n\n  按 Tab 可随时切回本地主题。"
            ))
            .style(Style::default().fg(swatch.r_muted()))
            .block(list_block(format!(" ✦ 社区市场 · 排序: {sort_label} ")));
            frame.render_widget(msg, rows[1]);
        }
        GalleryLoadState::Failed(err) => {
            let msg = Paragraph::new(format!(
                "\n  暂无法加载社区市场索引：\n  {err}\n\n  可检查网络/代理或运行 `kiseki gallery refresh`，按 Tab 返回本地主题。"
            ))
            .style(Style::default().fg(swatch.r_muted()))
            .wrap(Wrap { trim: true })
            .block(list_block(" ✦ 社区市场 (离线) ".to_string()));
            frame.render_widget(msg, rows[1]);
        }
        GalleryLoadState::Ready(all) => {
            let visible = session.visible_gallery_indices();
            let block = list_block(format!(
                " ✦ 社区市场 ({}/{}) · 排序: {} [s] ",
                visible.len(),
                all.len(),
                sort_label
            ));
            if visible.is_empty() {
                let empty = Paragraph::new(if all.is_empty() {
                    "\n  社区市场目前还没有收录主题。\n  运行 `kiseki theme init my-theme` 成为首批创作者！"
                } else {
                    "\n  未找到匹配的社区主题，按 Esc 清除搜索词。"
                })
                .style(Style::default().fg(swatch.r_muted()))
                .block(block);
                frame.render_widget(empty, rows[1]);
                return;
            }

            let mut selected_row = None;
            let list_items: Vec<ListItem<'static>> = visible
                .iter()
                .enumerate()
                .map(|(row_idx, &item_idx)| {
                    let item = &all[item_idx];
                    let is_cur = item_idx == session.gallery_cursor();
                    if is_cur {
                        selected_row = Some(row_idx);
                    }
                    let pointer = if is_cur { "▸ " } else { "  " };
                    let mark = if item.installed { "✓ " } else { "+ " };
                    let mark_style = if item.installed {
                        Style::default().fg(swatch.r_green())
                    } else {
                        Style::default().fg(swatch.r_secondary())
                    };
                    let id_style = if is_cur {
                        Style::default()
                            .fg(swatch.r_accent())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(swatch.r_fg())
                    };

                    let line = vec![
                        Span::styled(pointer, Style::default().fg(swatch.r_accent())),
                        Span::styled(mark, mark_style),
                        Span::styled(format!("{:<18}", item.id), id_style),
                        Span::styled("█", Style::default().fg(item.swatch.r_accent())),
                        Span::styled("█", Style::default().fg(item.swatch.r_secondary())),
                        Span::styled("█", Style::default().fg(item.swatch.r_star())),
                        Span::raw("  "),
                        Span::styled(item.name.clone(), Style::default().fg(swatch.r_fg())),
                        Span::styled(
                            format!("  ★{} ↓{}", item.stars, item.downloads),
                            Style::default().fg(swatch.r_muted()),
                        ),
                    ];
                    ListItem::new(Line::from(line))
                })
                .collect();

            let list = List::new(list_items)
                .block(block)
                .highlight_style(Style::default().bg(swatch.r_surface()));
            let mut state = ListState::default();
            state.select(selected_row);
            frame.render_stateful_widget(list, rows[1], &mut state);
        }
    }
}

fn draw_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &PickerSession,
    search_mode: bool,
    swatch: &ThemeSwatch,
) {
    let hint = if search_mode {
        "输入关键词筛选 · ↑/↓ 移动 · Enter 确认筛选 · Tab 切页 · Esc 退出搜索"
    } else if session.tab() == PickerTab::Gallery {
        "↑/↓ (j/k) 浏览 · Enter 安装并启用 · s 切换排序 · / 搜索 · Tab 回本地 · q/Esc 退出"
    } else if session.gallery_enabled() {
        "↑/↓ (j/k) 选择 · Enter 切换 · Tab 逛社区市场 · / 搜索 · Space 预览 · q/Esc 取消"
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
