//! Strongly typed theme model (RFC-0002).
//!
//! `manifest.json` describes package identity and provenance; `tokens.json`
//! holds the five composable token layers. Both deserialize straight into the
//! types in this module, and every optional field is genuinely optional so a
//! community theme can ship only L1.

use std::collections::BTreeMap;

use kiseki_types::{Appearance, Color, RenderingProfile, ThemeId};
use serde::{Deserialize, Serialize};

/// Current schema version understood by this build.
pub const SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Declares which of the five token layers a theme actually authors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayerKind {
    Palette,
    Syntax,
    Ui,
    Atmosphere,
    Typography,
    Overrides,
}

impl LayerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Palette => "palette",
            Self::Syntax => "syntax",
            Self::Ui => "ui",
            Self::Atmosphere => "atmosphere",
            Self::Typography => "typography",
            Self::Overrides => "overrides",
        }
    }
}

/// Where a theme's artwork and shader code came from, for Gallery auditing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WallpaperOrigin {
    Original,
    Cc0,
    CcBy,
    Licensed,
    None,
}

/// Provenance of the shader templates bundled in a theme pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShaderOrigin {
    Original,
    MitAdapted,
    None,
}

/// Asset origin declarations (RFC-0004 §5).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper_origin: Option<WallpaperOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader_origin: Option<ShaderOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_repo: Option<String>,
}

/// A theme author entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// `manifest.json` — package identity, appearance and provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeManifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,
    pub schema_version: u32,
    pub id: ThemeId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub version: String,
    pub appearance: Appearance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_theme: Option<ThemeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub authors: Vec<Author>,
    pub license: String,
    pub credits_file: String,
    pub layers: Vec<LayerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

impl ThemeManifest {
    /// True when the manifest declares the given layer.
    pub fn has_layer(&self, layer: LayerKind) -> bool {
        self.layers.contains(&layer)
    }
}

// ---------------------------------------------------------------------------
// L1 palette
// ---------------------------------------------------------------------------

/// Background elevation ramp, darkest (`crust`) to most elevated (`surface2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceTokens {
    pub crust: Color,
    pub mantle: Color,
    pub base: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub surface2: Color,
    pub overlay0: Color,
    pub overlay1: Color,
}

/// Text hierarchy tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextTokens {
    pub primary: Color,
    pub secondary: Color,
    pub muted: Color,
    pub inverse: Color,
}

/// The theme's identity colors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccentTokens {
    pub primary: Color,
    pub secondary: Color,
    pub tertiary: Color,
}

/// Semantic state colors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SemanticTokens {
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub hint: Color,
}

/// Cursor and selection tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionTokens {
    pub cursor: Color,
    pub cursor_text: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub match_bg: Color,
    pub match_fg: Color,
}

/// Standard 16-color ANSI palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnsiTokens {
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
    pub bright_black: Color,
    pub bright_red: Color,
    pub bright_green: Color,
    pub bright_yellow: Color,
    pub bright_blue: Color,
    pub bright_magenta: Color,
    pub bright_cyan: Color,
    pub bright_white: Color,
}

impl AnsiTokens {
    /// The 16 palette entries in ANSI index order (`0..=15`).
    pub fn as_indexed(&self) -> [Color; 16] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.magenta,
            self.cyan,
            self.white,
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_magenta,
            self.bright_cyan,
            self.bright_white,
        ]
    }

    /// Named colors that must clear the WCAG contrast floor (RFC-0002 §4).
    pub fn contrast_checked(&self) -> [(&'static str, Color); 6] {
        [
            ("red", self.red),
            ("green", self.green),
            ("yellow", self.yellow),
            ("blue", self.blue),
            ("magenta", self.magenta),
            ("cyan", self.cyan),
        ]
    }
}

/// L1 — the only mandatory layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaletteTokens {
    pub surfaces: SurfaceTokens,
    pub text: TextTokens,
    pub accents: AccentTokens,
    pub semantic: SemanticTokens,
    pub interaction: InteractionTokens,
    pub ansi: AnsiTokens,
}

// ---------------------------------------------------------------------------
// L2 syntax / diff / ui
// ---------------------------------------------------------------------------

/// The 19 semantic syntax slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyntaxTokens {
    pub keyword: Color,
    pub operator: Color,
    pub function: Color,
    pub method: Color,
    #[serde(rename = "type")]
    pub type_: Color,
    pub variable: Color,
    pub parameter: Color,
    pub property: Color,
    pub string: Color,
    pub escape: Color,
    pub number: Color,
    pub boolean: Color,
    pub constant: Color,
    pub comment: Color,
    pub punctuation: Color,
    pub tag: Color,
    pub attribute: Color,
    pub macro_: Color,
    pub invalid: Color,
}

/// Diff highlight tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffTokens {
    pub add_fg: Color,
    pub add_bg: Color,
    pub delete_fg: Color,
    pub delete_bg: Color,
    pub change_fg: Color,
    pub change_bg: Color,
    pub inline_add_bg: Color,
    pub inline_delete_bg: Color,
}

/// Window chrome tokens (tabs, frames, statusline, panels).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiTokens {
    pub window_frame: Color,
    pub window_frame_unfocused: Color,
    pub tab_bar_bg: Color,
    pub tab_bar_unfocused_bg: Color,
    pub tab_active_bg: Color,
    pub tab_active_fg: Color,
    pub tab_inactive_bg: Color,
    pub tab_inactive_fg: Color,
    pub statusline_bg: Color,
    pub statusline_fg: Color,
    pub border: Color,
    pub border_focused: Color,
    pub panel_bg: Color,
}

// ---------------------------------------------------------------------------
// L3 atmosphere
// ---------------------------------------------------------------------------

/// Window transparency, blur and padding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowAtmosphere {
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unfocused_opacity: Option<f64>,
    #[serde(default)]
    pub blur: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur_radius: Option<u8>,
    #[serde(default)]
    pub use_mica: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
}

fn default_opacity() -> f64 {
    1.0
}

/// Horizontal/vertical terminal padding in points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Padding {
    pub x: u8,
    pub y: u8,
}

/// Which side of the canvas stays readable for code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SafeZoneSide {
    Left,
    Right,
    Center,
    None,
}

impl SafeZoneSide {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Center => "center",
            Self::None => "none",
        }
    }
}

/// Declared text-safe region of the wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeSafeZone {
    pub side: SafeZoneSide,
    pub start_x: f64,
    pub end_x: f64,
}

/// Where the wallpaper sits on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WallpaperAlignment {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl WallpaperAlignment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }
}

/// How the wallpaper is scaled into the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperFit {
    Cover,
    Contain,
    Fill,
}

impl WallpaperFit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
            Self::Fill => "fill",
        }
    }
}

/// Alternative wallpaper assets for portrait/compact windows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WallpaperVariants {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<String>,
}

/// Wallpaper declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WallpaperAtmosphere {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unfocused_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<WallpaperAlignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<WallpaperFit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_safe_zone: Option<CodeSafeZone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<WallpaperVariants>,
}

/// A theme-declared shader parameter value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShaderParamValue {
    Number(f64),
    Bool(bool),
    Color(Color),
}

impl ShaderParamValue {
    /// Renders the value as a GLSL/HLSL literal.
    pub fn to_glsl_literal(&self) -> String {
        match self {
            Self::Number(value) => format!("{value:.4}"),
            Self::Bool(value) => u8::from(*value).to_string(),
            Self::Color(color) => {
                let (r, g, b) = color.to_unit_rgb();
                format!("vec3({r:.6}, {g:.6}, {b:.6})")
            }
        }
    }
}

/// Parameterized shader declaration (RFC-0004 §1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShaderAtmosphere {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glsl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hlsl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<RenderingProfile>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ShaderParamValue>,
}

/// L3 — immersive atmosphere layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AtmosphereTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowAtmosphere>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<WallpaperAtmosphere>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader: Option<ShaderAtmosphere>,
}

// ---------------------------------------------------------------------------
// L4 typography
// ---------------------------------------------------------------------------

/// Declared typography character of a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TypographyStyle {
    #[serde(rename = "8bit")]
    EightBit,
    Code,
    Maple,
    Modern,
    Custom,
}

impl TypographyStyle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EightBit => "8bit",
            Self::Code => "code",
            Self::Maple => "maple",
            Self::Modern => "modern",
            Self::Custom => "custom",
        }
    }
}

/// Font weight keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    Thin,
    Light,
    Normal,
    Medium,
    Semibold,
    Bold,
}

impl FontWeight {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Thin => "thin",
            Self::Light => "light",
            Self::Normal => "normal",
            Self::Medium => "medium",
            Self::Semibold => "semibold",
            Self::Bold => "bold",
        }
    }

    /// CSS/OpenType numeric weight.
    pub const fn numeric(self) -> u16 {
        match self {
            Self::Thin => 100,
            Self::Light => 300,
            Self::Normal => 400,
            Self::Medium => 500,
            Self::Semibold => 600,
            Self::Bold => 700,
        }
    }
}

/// L4 — semantic font stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TypographyTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<TypographyStyle>,
    pub families: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<FontWeight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ligatures: Option<bool>,
}

// ---------------------------------------------------------------------------
// L5 safe overrides
// ---------------------------------------------------------------------------

/// Cursor shape a terminal should render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorShape {
    Block,
    BlockHollow,
    Bar,
    Underline,
}

impl CursorShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::BlockHollow => "block-hollow",
            Self::Bar => "bar",
            Self::Underline => "underline",
        }
    }

    /// Windows Terminal's `cursorShape` vocabulary.
    pub const fn windows_terminal(self) -> &'static str {
        match self {
            Self::Block => "filledBox",
            Self::BlockHollow => "emptyBox",
            Self::Bar => "bar",
            Self::Underline => "underscore",
        }
    }
}

/// macOS titlebar treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MacosTitlebarStyle {
    Transparent,
    Tabs,
    Native,
}

impl MacosTitlebarStyle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transparent => "transparent",
            Self::Tabs => "tabs",
            Self::Native => "native",
        }
    }
}

/// Terminal visual overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_shape: Option<CursorShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_blink: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos_titlebar_style: Option<MacosTitlebarStyle>,
}

/// Built-in safe prompt skeleton names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptPreset {
    TacticalBar,
    PixelFrame,
    CinematicLine,
    ShrineRibbon,
    Minimal,
}

impl PromptPreset {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TacticalBar => "tactical-bar",
            Self::PixelFrame => "pixel-frame",
            Self::CinematicLine => "cinematic-line",
            Self::ShrineRibbon => "shrine-ribbon",
            Self::Minimal => "minimal",
        }
    }
}

/// Prompt segment color overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptSegmentColors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge_bg: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge_fg: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_bg: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_fg: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_bg: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_fg: Option<Color>,
}

/// Declarative prompt decoration. Deliberately has no field that could carry
/// an executable `command` segment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<PromptPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console_title_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_colors: Option<PromptSegmentColors>,
}

/// fzf border style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FzfBorder {
    Sharp,
    Rounded,
    Bold,
    Double,
    None,
}

impl FzfBorder {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sharp => "sharp",
            Self::Rounded => "rounded",
            Self::Bold => "bold",
            Self::Double => "double",
            Self::None => "none",
        }
    }
}

/// fzf glyph overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FzfOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<FzfBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
}

/// Static fastfetch field label renames.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FastfetchFieldLabels {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime: Option<String>,
}

/// fastfetch presentation overrides (no `command` modules, ever).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FastfetchOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_labels: Option<FastfetchFieldLabels>,
}

/// L5 — strictly declarative, zero-code-execution overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverrideTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalOverrides>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptOverrides>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fzf: Option<FzfOverrides>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fastfetch: Option<FastfetchOverrides>,
}

// ---------------------------------------------------------------------------
// tokens.json root
// ---------------------------------------------------------------------------

/// `tokens.json` — the five composable token layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeTokens {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,
    pub schema_version: u32,
    pub palette: PaletteTokens,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax: Option<PartialSyntaxTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<PartialDiffTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<PartialUiTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atmosphere: Option<AtmosphereTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typography: Option<TypographyTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<OverrideTokens>,
}

/// A theme pack as loaded from disk: manifest plus tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemePack {
    pub manifest: ThemeManifest,
    pub tokens: ThemeTokens,
}

// ---------------------------------------------------------------------------
// Partial L2 shapes (authoring-time optionality)
// ---------------------------------------------------------------------------

macro_rules! partial_color_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident { $( $field:ident ),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $field: Option<Color>,
            )*
        }
    };
}

partial_color_struct! {
    /// Authoring-time syntax layer; omitted slots derive from L1.
    pub struct PartialSyntaxTokens {
        keyword, operator, function, method, r#type, variable, parameter, property,
        string, escape, number, boolean, constant, comment, punctuation, tag,
        attribute, r#macro, invalid
    }
}

partial_color_struct! {
    /// Authoring-time diff layer; omitted slots derive from L1.
    pub struct PartialDiffTokens {
        add_fg, add_bg, delete_fg, delete_bg, change_fg, change_bg, inline_add_bg, inline_delete_bg
    }
}

partial_color_struct! {
    /// Authoring-time chrome layer; omitted slots derive from L1.
    pub struct PartialUiTokens {
        window_frame, window_frame_unfocused, tab_bar_bg, tab_bar_unfocused_bg,
        tab_active_bg, tab_active_fg, tab_inactive_bg, tab_inactive_fg,
        statusline_bg, statusline_fg, border, border_focused, panel_bg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_TOKENS: &str = r##"{
      "schemaVersion": 1,
      "palette": {
        "surfaces": {
          "crust": "#070506", "mantle": "#0E0B0C", "base": "#0B090A",
          "surface0": "#171214", "surface1": "#241418", "surface2": "#55383B",
          "overlay0": "#6F6462", "overlay1": "#A59A93"
        },
        "text": { "primary": "#ECE4DA", "secondary": "#DDD3C9", "muted": "#8C7D78", "inverse": "#0B090A" },
        "accents": { "primary": "#F06A22", "secondary": "#D8262E", "tertiary": "#62C6F2" },
        "semantic": { "error": "#F0413C", "warning": "#F5C96A", "success": "#3FE08A", "info": "#62C6F2", "hint": "#FF8A3D" },
        "interaction": {
          "cursor": "#F06A22", "cursorText": "#0B090A", "selectionBg": "#4E1218",
          "selectionFg": "#FFF7F2", "matchBg": "#4A2410", "matchFg": "#FFC08F"
        },
        "ansi": {
          "black": "#171214", "red": "#D8262E", "green": "#3FE08A", "yellow": "#F06A22",
          "blue": "#62C6F2", "magenta": "#E05A6D", "cyan": "#FF8A3D", "white": "#DDD3C9",
          "brightBlack": "#7D706B", "brightRed": "#F0413C", "brightGreen": "#86F7BB",
          "brightYellow": "#F5C96A", "brightBlue": "#8FD8FA", "brightMagenta": "#FFA58F",
          "brightCyan": "#FFC08F", "brightWhite": "#FFF7F2"
        }
      }
    }"##;

    #[test]
    fn minimal_l1_only_tokens_parse() {
        let tokens: ThemeTokens = serde_json::from_str(MINIMAL_TOKENS).unwrap();
        assert_eq!(tokens.palette.surfaces.base, Color::parse_hex("#0B090A").unwrap());
        assert!(tokens.syntax.is_none());
        assert!(tokens.atmosphere.is_none());
        assert!(tokens.overrides.is_none());
    }

    #[test]
    fn ansi_index_order_matches_terminal_convention() {
        let tokens: ThemeTokens = serde_json::from_str(MINIMAL_TOKENS).unwrap();
        let indexed = tokens.palette.ansi.as_indexed();
        assert_eq!(indexed[0], tokens.palette.ansi.black);
        assert_eq!(indexed[1], tokens.palette.ansi.red);
        assert_eq!(indexed[15], tokens.palette.ansi.bright_white);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = MINIMAL_TOKENS.replace("\"schemaVersion\": 1,", "\"schemaVersion\": 1, \"evil\": 1,");
        assert!(serde_json::from_str::<ThemeTokens>(&json).is_err());
    }

    #[test]
    fn shader_params_are_typed() {
        let json = r##"{
          "schemaVersion": 1,
          "palette": { "surfaces": { "crust": "#000000", "mantle": "#000000", "base": "#000000",
            "surface0": "#000000", "surface1": "#000000", "surface2": "#000000",
            "overlay0": "#000000", "overlay1": "#000000" },
            "text": { "primary": "#FFFFFF", "secondary": "#FFFFFF", "muted": "#808080", "inverse": "#000000" },
            "accents": { "primary": "#FF0000", "secondary": "#00FF00", "tertiary": "#0000FF" },
            "semantic": { "error": "#FF0000", "warning": "#FFFF00", "success": "#00FF00", "info": "#0000FF", "hint": "#FF00FF" },
            "interaction": { "cursor": "#FFFFFF", "cursorText": "#000000", "selectionBg": "#000080",
              "selectionFg": "#FFFFFF", "matchBg": "#404000", "matchFg": "#FFFF00" },
            "ansi": { "black": "#000000", "red": "#FF0000", "green": "#00FF00", "yellow": "#FFFF00",
              "blue": "#0000FF", "magenta": "#FF00FF", "cyan": "#00FFFF", "white": "#FFFFFF",
              "brightBlack": "#808080", "brightRed": "#FF8080", "brightGreen": "#80FF80",
              "brightYellow": "#FFFF80", "brightBlue": "#8080FF", "brightMagenta": "#FF80FF",
              "brightCyan": "#80FFFF", "brightWhite": "#FFFFFF" }
          },
          "atmosphere": { "shader": { "params": { "scanPeriodSec": 7.5, "glitch": true, "tint": "#F06A22" } } }
        }"##;
        let tokens: ThemeTokens = serde_json::from_str(json).unwrap();
        let params = &tokens.atmosphere.unwrap().shader.unwrap().params;
        assert!(matches!(params["scanPeriodSec"], ShaderParamValue::Number(_)));
        assert!(matches!(params["glitch"], ShaderParamValue::Bool(true)));
        assert!(matches!(params["tint"], ShaderParamValue::Color(_)));
        assert_eq!(params["scanPeriodSec"].to_glsl_literal(), "7.5000");
        assert_eq!(params["glitch"].to_glsl_literal(), "1");
    }

    #[test]
    fn cursor_shape_maps_to_windows_terminal_vocabulary() {
        assert_eq!(CursorShape::Block.windows_terminal(), "filledBox");
        assert_eq!(CursorShape::BlockHollow.windows_terminal(), "emptyBox");
        assert_eq!(CursorShape::Underline.windows_terminal(), "underscore");
    }

    #[test]
    fn manifest_roundtrips() {
        let json = r##"{
          "schemaVersion": 1,
          "id": "nerv-eva",
          "name": "NERV UNIT-02",
          "version": "2.0.0",
          "appearance": "dark",
          "authors": [{ "name": "nerv-base", "url": "https://github.com/nerv-base" }],
          "license": "MIT",
          "creditsFile": "CREDITS.md",
          "layers": ["palette", "syntax", "ui"],
          "provenance": { "wallpaperOrigin": "original", "shaderOrigin": "original" }
        }"##;
        let manifest: ThemeManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.id.as_str(), "nerv-eva");
        assert!(manifest.has_layer(LayerKind::Palette));
        assert!(!manifest.has_layer(LayerKind::Atmosphere));
        assert_eq!(
            manifest.provenance.unwrap().wallpaper_origin,
            Some(WallpaperOrigin::Original)
        );
    }
}
