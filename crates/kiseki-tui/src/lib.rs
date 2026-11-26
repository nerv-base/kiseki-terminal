//! Interactive Ratatui theme picker, one-Enter init wizard, and Kiseki 8-bit logo.
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

/// One row in the theme picker or init wizard.
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

/// What the picker is asking the CLI to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerAction {
    /// Commit the theme (Enter).
    Select(String),
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
    Char(char),
}

/// Picker state machine. Rendering-agnostic on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerSession {
    items: Vec<PickerItem>,
    cursor: usize,
    query: String,
    action: PickerAction,
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
        }
    }

    pub fn items(&self) -> &[PickerItem] {
        &self.items
    }

    pub fn cursor(&self) -> usize {
        self.cursor
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

    /// The item under the cursor.
    pub fn current(&self) -> Option<&PickerItem> {
        self.items.get(self.cursor)
    }

    /// Indices currently visible under the search query.
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

    pub fn move_up(&mut self) {
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

    pub fn move_down(&mut self) {
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

    pub fn move_home(&mut self) {
        if let Some(first) = self.visible_indices().first().copied() {
            self.cursor = first;
            self.emit_preview();
        }
    }

    pub fn move_end(&mut self) {
        if let Some(last) = self.visible_indices().last().copied() {
            self.cursor = last;
            self.emit_preview();
        }
    }

    fn emit_preview(&mut self) {
        if let Some(item) = self.current() {
            self.action = PickerAction::Preview(item.id.clone());
        }
    }

    pub fn preview(&mut self) -> PickerAction {
        self.emit_preview();
        self.action.clone()
    }

    pub fn select(&mut self) -> PickerAction {
        if let Some(item) = self.current() {
            self.action = PickerAction::Select(item.id.clone());
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

/// Runs the Ratatui interactive theme picker.
///
/// Non-TTY callers get [`PickerAction::Cancel`] so scripts never block.
pub fn run_picker(items: Vec<PickerItem>, active: Option<&str>) -> Result<PickerAction> {
    let mut session = PickerSession::new(items, active);
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Ok(PickerAction::Cancel);
    }
    picker::run(&mut session).map(|_| session.action().clone())
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
    pub install_fonts: bool,
}

impl InitWizardChoices {
    /// Safe defaults for CI and non-interactive runs (ADR-006).
    pub fn non_interactive(input: &InitWizardInput) -> Self {
        let theme = input
            .default_theme
            .clone()
            .or_else(|| input.themes.first().map(|item| item.id.clone()))
            .unwrap_or_else(|| "nerv-eva".to_string());
        let profile = input.default_profile.unwrap_or(if input.default_render_only {
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
            install_fonts: false,
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
        assert!(!choices.install_fonts);
    }

    #[test]
    fn empty_input_still_produces_a_theme() {
        let choices = InitWizardChoices::non_interactive(&InitWizardInput::default());
        assert_eq!(choices.theme, "nerv-eva");
        assert_eq!(choices.prompt_policy, PromptPolicy::ManagedTemplate);
        assert_eq!(choices.profile, RenderingProfile::Balanced);
    }
}
