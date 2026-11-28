//! Interactive Ratatui theme picker (with optional Gallery tab), one-Enter
//! init wizard, and Kiseki 8-bit logo.
//!
//! Zero Nerd Font dependency: all borders, swatches and logos use standard
//! Unicode box-drawing / half-block characters with an automatic pure-ASCII
//! fallback when `NO_COLOR` or `TERM=dumb` is active.

pub mod logo;
pub mod picker;
pub mod wizard;

use std::io::IsTerminal;

use kiseki_types::{Appearance, PromptPolicy, RenderingProfile, Result};

pub use logo::{render_mini_cli_logo, ThemeSwatch};
pub use wizard::{InitWizardSession, WizardEvent, WizardOutcome};

/// Enables Windows console virtual terminal processing so ANSI/TrueColor
/// output renders cleanly in classic Windows PowerShell 5.1 (`conhost.exe`).
pub fn enable_ansi_support() {
    #[cfg(windows)]
    {
        let _ = crossterm::ansi_support::supports_ansi();
    }
}

/// Active tab in the interactive picker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PickerTab {
    #[default]
    Local,
    Gallery,
}

/// Sort mode inside the Gallery tab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GallerySortMode {
    #[default]
    Trending,
    Downloads,
    Stars,
    Updated,
}

impl GallerySortMode {
    pub const fn next(self) -> Self {
        match self {
            Self::Trending => Self::Downloads,
            Self::Downloads => Self::Stars,
            Self::Stars => Self::Updated,
            Self::Updated => Self::Trending,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Trending => "热门",
            Self::Downloads => "下载",
            Self::Stars => "Star",
            Self::Updated => "最新",
        }
    }
}

/// One row in the local theme picker or init wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerItem {
    /// Theme identifier (`nerv-eva`).
    pub id: String,
    /// Display name (`NERV UNIT-02`).
    pub name: String,
    /// One-line tagline / subtitle.
    pub subtitle: Option<String>,
    pub appearance: Appearance,
    /// True when this is the currently active theme.
    pub active: bool,
    /// Optional short badge (`built-in`, `02`).
    pub badge: Option<String>,
    /// Palette colors used to live-tint the TUI & 8-bit Kiseki logo.
    pub swatch: ThemeSwatch,
}

impl PickerItem {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            subtitle: None,
            appearance: Appearance::Dark,
            active: false,
            badge: None,
            swatch: ThemeSwatch::default(),
        }
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn with_appearance(mut self, appearance: Appearance) -> Self {
        self.appearance = appearance;
        self
    }

    pub fn with_swatch(mut self, swatch: ThemeSwatch) -> Self {
        self.swatch = swatch;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

/// One community theme entry shown inside the TUI `Gallery` tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryPickerItem {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub appearance: Appearance,
    pub version: String,
    pub author: String,
    pub repo: String,
    pub stars: u64,
    pub downloads: u64,
    /// `trendingScore * 1000` rounded to integer for deterministic `Ord`/`Eq`.
    pub trending_milli: u64,
    pub updated_at: String,
    pub has_wallpaper: bool,
    pub has_shader: bool,
    pub wcag_grade: String,
    pub installed: bool,
    pub swatch: ThemeSwatch,
}

/// Background load state for the TUI `Gallery` tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GalleryLoadState {
    #[default]
    Idle,
    Loading,
    Ready(Vec<GalleryPickerItem>),
    Failed(String),
}

/// What the picker is asking the CLI to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerAction {
    /// Commit a locally available theme (Enter in Local tab).
    Select(String),
    /// Install (if needed) and activate a community theme (Enter in Gallery tab).
    InstallGallery(String),
    /// Live-preview the theme without persisting (Space / cursor move).
    Preview(String),
    /// Exit without changing anything (Esc / q / Ctrl+C).
    Cancel,
    /// Nothing yet.
    None,
}

/// Keyboard-independent input events, so the state machine is testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerInput {
    Up,
    Down,
    Home,
    End,
    Enter,
    Space,
    Cancel,
    Backspace,
    SwitchTab,
    CycleSort,
    Char(char),
}

/// Picker state machine. Rendering-agnostic on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerSession {
    items: Vec<PickerItem>,
    cursor: usize,
    query: String,
    action: PickerAction,
    tab: PickerTab,
    gallery_enabled: bool,
    gallery_state: GalleryLoadState,
    gallery_cursor: usize,
    gallery_sort: GallerySortMode,
}

impl PickerSession {
    /// Creates a session, placing the cursor on the active theme when present.
    pub fn new(items: Vec<PickerItem>, active: Option<&str>) -> Self {
        let cursor = active
            .and_then(|active| items.iter().position(|item| item.id == active))
            .unwrap_or(0);
        Self {
            items,
            cursor,
            query: String::new(),
            action: PickerAction::None,
            tab: PickerTab::Local,
            gallery_enabled: false,
            gallery_state: GalleryLoadState::Idle,
            gallery_cursor: 0,
            gallery_sort: GallerySortMode::Trending,
        }
    }

    pub fn with_gallery_enabled(mut self, enabled: bool) -> Self {
        self.gallery_enabled = enabled;
        self
    }

    pub fn items(&self) -> &[PickerItem] {
        &self.items
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn tab(&self) -> PickerTab {
        self.tab
    }

    pub fn gallery_enabled(&self) -> bool {
        self.gallery_enabled
    }

    pub fn gallery_state(&self) -> &GalleryLoadState {
        &self.gallery_state
    }

    pub fn gallery_cursor(&self) -> usize {
        self.gallery_cursor
    }

    pub fn gallery_sort(&self) -> GallerySortMode {
        self.gallery_sort
    }

    pub fn set_gallery_state(&mut self, state: GalleryLoadState) {
        self.gallery_state = state;
        self.gallery_cursor = self.visible_gallery_indices().first().copied().unwrap_or(0);
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn action(&self) -> &PickerAction {
        &self.action
    }

    pub fn take_action(&mut self) -> PickerAction {
        std::mem::replace(&mut self.action, PickerAction::None)
    }

    /// The local item under the cursor.
    pub fn current(&self) -> Option<&PickerItem> {
        self.items.get(self.cursor)
    }

    /// The gallery item under the cursor when Gallery items are loaded.
    pub fn current_gallery(&self) -> Option<&GalleryPickerItem> {
        match &self.gallery_state {
            GalleryLoadState::Ready(list) => list.get(self.gallery_cursor),
            _ => None,
        }
    }

    /// Active swatch across either tab.
    pub fn active_swatch(&self) -> ThemeSwatch {
        match self.tab {
            PickerTab::Local => self.current().map(|i| i.swatch).unwrap_or_default(),
            PickerTab::Gallery => self
                .current_gallery()
                .map(|i| i.swatch)
                .or_else(|| self.current().map(|i| i.swatch))
                .unwrap_or_default(),
        }
    }

    /// Indices currently visible in the Local tab under the search query.
    pub fn visible_indices(&self) -> Vec<usize> {
        if self.query.is_empty() {
            return (0..self.items.len()).collect();
        }
        let needle = self.query.to_ascii_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.id.to_ascii_lowercase().contains(&needle)
                    || item.name.to_ascii_lowercase().contains(&needle)
                    || item
                        .subtitle
                        .as_deref()
                        .is_some_and(|sub| sub.to_ascii_lowercase().contains(&needle))
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Sorted + filtered indices in the Gallery tab.
    pub fn visible_gallery_indices(&self) -> Vec<usize> {
        let GalleryLoadState::Ready(list) = &self.gallery_state else {
            return Vec::new();
        };
        let needle = self.query.trim().to_ascii_lowercase();
        let mut indices: Vec<usize> = list
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                needle.is_empty()
                    || item.id.to_ascii_lowercase().contains(&needle)
                    || item.name.to_ascii_lowercase().contains(&needle)
                    || item.subtitle.to_ascii_lowercase().contains(&needle)
                    || item.author.to_ascii_lowercase().contains(&needle)
                    || item.repo.to_ascii_lowercase().contains(&needle)
            })
            .map(|(idx, _)| idx)
            .collect();

        indices.sort_by(|&a, &b| {
            let lhs = &list[a];
            let rhs = &list[b];
            let ord = match self.gallery_sort {
                GallerySortMode::Trending => rhs
                    .trending_milli
                    .cmp(&lhs.trending_milli)
                    .then_with(|| rhs.downloads.cmp(&lhs.downloads))
                    .then_with(|| rhs.stars.cmp(&lhs.stars)),
                GallerySortMode::Downloads => rhs.downloads.cmp(&lhs.downloads),
                GallerySortMode::Stars => rhs.stars.cmp(&lhs.stars),
                GallerySortMode::Updated => rhs.updated_at.cmp(&lhs.updated_at),
            };
            ord.then_with(|| lhs.id.cmp(&rhs.id))
        });

        indices
    }

    pub fn move_up(&mut self) {
        match self.tab {
            PickerTab::Local => {
                let visible = self.visible_indices();
                if visible.is_empty() {
                    return;
                }
                let position = visible
                    .iter()
                    .position(|index| *index == self.cursor)
                    .unwrap_or(0);
                let next = if position == 0 {
                    visible.len() - 1
                } else {
                    position - 1
                };
                self.cursor = visible[next];
                self.emit_preview();
            }
            PickerTab::Gallery => {
                let visible = self.visible_gallery_indices();
                if visible.is_empty() {
                    return;
                }
                let position = visible
                    .iter()
                    .position(|index| *index == self.gallery_cursor)
                    .unwrap_or(0);
                let next = if position == 0 {
                    visible.len() - 1
                } else {
                    position - 1
                };
                self.gallery_cursor = visible[next];
                self.action = PickerAction::None;
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.tab {
            PickerTab::Local => {
                let visible = self.visible_indices();
                if visible.is_empty() {
                    return;
                }
                let position = visible
                    .iter()
                    .position(|index| *index == self.cursor)
                    .unwrap_or(0);
                let next = (position + 1) % visible.len();
                self.cursor = visible[next];
                self.emit_preview();
            }
            PickerTab::Gallery => {
                let visible = self.visible_gallery_indices();
                if visible.is_empty() {
                    return;
                }
                let position = visible
                    .iter()
                    .position(|index| *index == self.gallery_cursor)
                    .unwrap_or(0);
                let next = (position + 1) % visible.len();
                self.gallery_cursor = visible[next];
                self.action = PickerAction::None;
            }
        }
    }

    pub fn move_home(&mut self) {
        match self.tab {
            PickerTab::Local => {
                if let Some(first) = self.visible_indices().first().copied() {
                    self.cursor = first;
                    self.emit_preview();
                }
            }
            PickerTab::Gallery => {
                if let Some(first) = self.visible_gallery_indices().first().copied() {
                    self.gallery_cursor = first;
                }
                self.action = PickerAction::None;
            }
        }
    }

    pub fn move_end(&mut self) {
        match self.tab {
            PickerTab::Local => {
                if let Some(last) = self.visible_indices().last().copied() {
                    self.cursor = last;
                    self.emit_preview();
                }
            }
            PickerTab::Gallery => {
                if let Some(last) = self.visible_gallery_indices().last().copied() {
                    self.gallery_cursor = last;
                }
                self.action = PickerAction::None;
            }
        }
    }

    fn emit_preview(&mut self) {
        if let Some(item) = self.current() {
            self.action = PickerAction::Preview(item.id.clone());
        }
    }

    pub fn preview(&mut self) -> PickerAction {
        if self.tab == PickerTab::Local {
            self.emit_preview();
        } else {
            self.action = PickerAction::None;
        }
        self.action.clone()
    }

    pub fn select(&mut self) -> PickerAction {
        match self.tab {
            PickerTab::Local => {
                if let Some(item) = self.current() {
                    self.action = PickerAction::Select(item.id.clone());
                }
            }
            PickerTab::Gallery => {
                if let Some(item) = self.current_gallery() {
                    self.action = PickerAction::InstallGallery(item.id.clone());
                }
            }
        }
        self.action.clone()
    }

    pub fn cancel(&mut self) -> PickerAction {
        self.action = PickerAction::Cancel;
        self.action.clone()
    }

    /// Applies one input event and returns the resulting action.
    pub fn handle(&mut self, input: PickerInput) -> PickerAction {
        match input {
            PickerInput::Up => self.move_up(),
            PickerInput::Down => self.move_down(),
            PickerInput::Home => self.move_home(),
            PickerInput::End => self.move_end(),
            PickerInput::Enter => return self.select(),
            PickerInput::Space => return self.preview(),
            PickerInput::Cancel => return self.cancel(),
            PickerInput::SwitchTab => {
                if self.gallery_enabled {
                    self.tab = match self.tab {
                        PickerTab::Local => PickerTab::Gallery,
                        PickerTab::Gallery => PickerTab::Local,
                    };
                    if self.tab == PickerTab::Gallery {
                        let vis = self.visible_gallery_indices();
                        if !vis.contains(&self.gallery_cursor) {
                            self.gallery_cursor = vis.first().copied().unwrap_or(0);
                        }
                    }
                }
                self.action = PickerAction::None;
            }
            PickerInput::CycleSort => {
                if self.tab == PickerTab::Gallery {
                    self.gallery_sort = self.gallery_sort.next();
                    if let Some(first) = self.visible_gallery_indices().first().copied() {
                        self.gallery_cursor = first;
                    }
                }
                self.action = PickerAction::None;
            }
            PickerInput::Backspace => {
                self.query.pop();
                self.action = PickerAction::None;
            }
            PickerInput::Char(ch) => {
                self.query.push(ch);
                self.action = PickerAction::None;
            }
        }
        self.action.clone()
    }
}

/// Runs the Ratatui interactive theme picker (local tab only).
///
/// Non-TTY callers get [`PickerAction::Cancel`] so scripts never block.
pub fn run_picker(items: Vec<PickerItem>, active: Option<&str>) -> Result<PickerAction> {
    let mut session = PickerSession::new(items, active);
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Ok(PickerAction::Cancel);
    }
    picker::run(&mut session, None).map(|_| session.action().clone())
}

/// Runs the interactive theme picker with a lazy-loaded `Gallery` tab.
///
/// `loader` is only invoked if the user explicitly presses `Tab` to visit the
/// Gallery tab, preserving zero-network startup on the Local tab.
pub fn run_picker_with_gallery<F>(
    items: Vec<PickerItem>,
    active: Option<&str>,
    loader: F,
) -> Result<PickerAction>
where
    F: FnOnce() -> Result<Vec<GalleryPickerItem>> + Send + 'static,
{
    let mut session = PickerSession::new(items, active).with_gallery_enabled(true);
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Ok(PickerAction::Cancel);
    }
    picker::run(&mut session, Some(Box::new(loader))).map(|_| session.action().clone())
}

/// Everything `init` detected before presenting the wizard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitWizardInput {
    pub themes: Vec<PickerItem>,
    pub detected_terminal: Option<String>,
    pub detected_shell: Option<String>,
    pub v1_detected: bool,
    pub user_prompt_config: bool,
    pub default_render_only: bool,
    pub default_theme: Option<String>,
    pub default_profile: Option<RenderingProfile>,
}

/// The choices `init` returns to the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitWizardChoices {
    pub theme: String,
    pub profile: RenderingProfile,
    pub prompt_policy: PromptPolicy,
    pub render_only: bool,
    pub aliases: bool,
}

impl InitWizardChoices {
    /// Safe defaults for CI and non-interactive runs (ADR-006).
    pub fn non_interactive(input: &InitWizardInput) -> Self {
        let theme = input
            .default_theme
            .clone()
            .or_else(|| input.themes.first().map(|item| item.id.clone()))
            .unwrap_or_else(|| "nerv-eva".to_string());
        let profile = input
            .default_profile
            .unwrap_or(if input.default_render_only {
                RenderingProfile::Static
            } else {
                RenderingProfile::Balanced
            });
        Self {
            theme,
            profile,
            // Never overwrite a hand-authored prompt config silently.
            prompt_policy: if input.user_prompt_config {
                PromptPolicy::PaletteOnly
            } else {
                PromptPolicy::ManagedTemplate
            },
            render_only: input.default_render_only,
            aliases: false,
        }
    }
}

/// Runs the one-Enter Ratatui init wizard.
///
/// Returns `Ok(None)` when the user cancels (`Esc` / `q` / `Ctrl+C`) so `init`
/// exits cleanly with zero host writes. Non-TTY callers receive safe
/// non-interactive defaults wrapped in `Some(...)`.
pub fn run_init_wizard(input: InitWizardInput) -> Result<Option<InitWizardChoices>> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Ok(Some(InitWizardChoices::non_interactive(&input)));
    }
    wizard::run(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<PickerItem> {
        vec![
            PickerItem::new("nerv-eva", "NERV UNIT-02")
                .with_subtitle("tactical")
                .active(true),
            PickerItem::new("kyoto-sakura", "Kyoto Sakura").with_appearance(Appearance::Light),
            PickerItem::new("shinkai-rain", "Shinkai Rain"),
        ]
    }

    fn gallery_items() -> Vec<GalleryPickerItem> {
        vec![
            GalleryPickerItem {
                id: "tokyo-midnight".into(),
                name: "Tokyo Midnight".into(),
                subtitle: "City pop indigo".into(),
                appearance: Appearance::Dark,
                version: "1.2.0".into(),
                author: "Alice".into(),
                repo: "alice/tokyo-midnight".into(),
                stars: 180,
                downloads: 420,
                trending_milli: 4800,
                updated_at: "2026-11-28T00:00:00Z".into(),
                has_wallpaper: true,
                has_shader: true,
                wcag_grade: "AA".into(),
                installed: false,
                swatch: ThemeSwatch::default(),
            },
            GalleryPickerItem {
                id: "snow-lotus".into(),
                name: "Snow Lotus".into(),
                subtitle: "Clean daylight".into(),
                appearance: Appearance::Light,
                version: "1.0.0".into(),
                author: "Bob".into(),
                repo: "bob/snow-lotus".into(),
                stars: 450,
                downloads: 120,
                trending_milli: 3900,
                updated_at: "2026-11-20T00:00:00Z".into(),
                has_wallpaper: false,
                has_shader: false,
                wcag_grade: "AAA".into(),
                installed: true,
                swatch: ThemeSwatch::default(),
            },
        ]
    }

    #[test]
    fn session_starts_on_the_active_theme() {
        let session = PickerSession::new(items(), Some("shinkai-rain"));
        assert_eq!(session.cursor(), 2);
        assert_eq!(session.current().unwrap().id, "shinkai-rain");
        let session = PickerSession::new(items(), Some("missing"));
        assert_eq!(session.cursor(), 0);
    }

    #[test]
    fn navigation_wraps_and_previews() {
        let mut session = PickerSession::new(items(), None);
        assert_eq!(
            session.handle(PickerInput::Down),
            PickerAction::Preview("kyoto-sakura".into())
        );
        assert_eq!(
            session.handle(PickerInput::Down),
            PickerAction::Preview("shinkai-rain".into())
        );
        assert_eq!(
            session.handle(PickerInput::Down),
            PickerAction::Preview("nerv-eva".into())
        );
        assert_eq!(
            session.handle(PickerInput::Up),
            PickerAction::Preview("shinkai-rain".into())
        );
        assert_eq!(
            session.handle(PickerInput::Home),
            PickerAction::Preview("nerv-eva".into())
        );
        assert_eq!(
            session.handle(PickerInput::End),
            PickerAction::Preview("shinkai-rain".into())
        );
    }

    #[test]
    fn enter_selects_and_escape_cancels() {
        let mut session = PickerSession::new(items(), None);
        session.handle(PickerInput::Down);
        assert_eq!(
            session.handle(PickerInput::Enter),
            PickerAction::Select("kyoto-sakura".into())
        );
        assert_eq!(
            session.take_action(),
            PickerAction::Select("kyoto-sakura".into())
        );
        assert_eq!(session.take_action(), PickerAction::None);
        let mut session = PickerSession::new(items(), None);
        assert_eq!(session.handle(PickerInput::Cancel), PickerAction::Cancel);
    }

    #[test]
    fn gallery_tab_switches_sorts_and_emits_install_action() {
        let mut session = PickerSession::new(items(), None).with_gallery_enabled(true);
        assert_eq!(session.tab(), PickerTab::Local);
        session.handle(PickerInput::SwitchTab);
        assert_eq!(session.tab(), PickerTab::Gallery);

        session.set_gallery_state(GalleryLoadState::Ready(gallery_items()));
        assert_eq!(session.current_gallery().unwrap().id, "tokyo-midnight");

        // Trending -> Downloads -> Stars (snow-lotus has more stars).
        session.handle(PickerInput::CycleSort);
        assert_eq!(session.gallery_sort(), GallerySortMode::Downloads);
        session.handle(PickerInput::CycleSort);
        assert_eq!(session.gallery_sort(), GallerySortMode::Stars);
        assert_eq!(session.current_gallery().unwrap().id, "snow-lotus");

        assert_eq!(
            session.handle(PickerInput::Enter),
            PickerAction::InstallGallery("snow-lotus".into())
        );
    }

    #[test]
    fn search_filters_and_keeps_cursor_valid() {
        let mut session = PickerSession::new(items(), None);
        for ch in "sakura".chars() {
            session.handle(PickerInput::Char(ch));
        }
        assert_eq!(session.visible_indices(), vec![1]);
        assert_eq!(session.current().unwrap().id, "nerv-eva");
        session.handle(PickerInput::Down);
        assert_eq!(session.current().unwrap().id, "kyoto-sakura");
        for _ in 0..2 {
            session.handle(PickerInput::Backspace);
        }
        assert_eq!(session.query(), "saku");
    }

    #[test]
    fn non_interactive_defaults_are_safe() {
        let input = InitWizardInput {
            themes: items(),
            v1_detected: true,
            user_prompt_config: true,
            default_render_only: true,
            ..InitWizardInput::default()
        };
        let choices = InitWizardChoices::non_interactive(&input);
        assert_eq!(choices.theme, "nerv-eva");
        assert_eq!(choices.prompt_policy, PromptPolicy::PaletteOnly);
        assert!(choices.render_only);
        assert_eq!(choices.profile, RenderingProfile::Static);
        assert!(!choices.aliases);
    }

    #[test]
    fn empty_input_still_produces_a_theme() {
        let choices = InitWizardChoices::non_interactive(&InitWizardInput::default());
        assert_eq!(choices.theme, "nerv-eva");
        assert_eq!(choices.prompt_policy, PromptPolicy::ManagedTemplate);
        assert_eq!(choices.profile, RenderingProfile::Balanced);
    }
}
