//! Kiseki 8-bit pixel terminal window logo (adapted 1:1 from `assets/logo.svg`).
//!
//! Renders either as crisp TrueColor half-blocks (`▀▄█`, 22 cols × 11 rows) or
//! as a 100% pure-ASCII box fallback (`22 cols × 9 rows`) when color is
//! disabled or the terminal is constrained. Never depends on Nerd Font glyphs.

use kiseki_types::Color;
use ratatui::style::{Color as RColor, Modifier, Style};
use ratatui::text::{Line, Span};

/// Compact color palette used to tint the TUI chrome and Kiseki 8-bit logo live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSwatch {
    pub bg: (u8, u8, u8),
    pub surface: (u8, u8, u8),
    pub border: (u8, u8, u8),
    pub fg: (u8, u8, u8),
    pub muted: (u8, u8, u8),
    pub accent: (u8, u8, u8),
    pub secondary: (u8, u8, u8),
    pub tertiary: (u8, u8, u8),
    pub star: (u8, u8, u8),
    pub dot_red: (u8, u8, u8),
    pub dot_yellow: (u8, u8, u8),
    pub dot_green: (u8, u8, u8),
}

impl Default for ThemeSwatch {
    /// Starlight 8-Bit brand defaults matching `assets/logo.svg`.
    fn default() -> Self {
        Self {
            bg: (10, 15, 37),         // #0A0F25
            surface: (23, 32, 70),    // #172046
            border: (122, 162, 247),  // #7AA2F7
            fg: (232, 236, 255),      // #E8ECFF
            muted: (92, 101, 153),    // #5C6599
            accent: (255, 130, 184),  // #FF82B8
            secondary: (102, 240, 255), // #66F0FF
            tertiary: (180, 142, 254),  // #B48EFE
            star: (255, 229, 163),    // #FFE5A3
            dot_red: (255, 107, 139), // #FF6B8B
            dot_yellow: (255, 213, 128), // #FFD580
            dot_green: (114, 241, 184),  // #72F1B8
        }
    }
}

impl ThemeSwatch {
    #[allow(clippy::too_many_arguments)]
    pub fn from_theme_colors(
        bg: Color,
        surface: Color,
        border: Color,
        fg: Color,
        muted: Color,
        accent: Color,
        secondary: Color,
        tertiary: Color,
        star: Color,
        dot_red: Color,
        dot_yellow: Color,
        dot_green: Color,
    ) -> Self {
        Self {
            bg: bg.rgb(),
            surface: surface.rgb(),
            border: border.rgb(),
            fg: fg.rgb(),
            muted: muted.rgb(),
            accent: accent.rgb(),
            secondary: secondary.rgb(),
            tertiary: tertiary.rgb(),
            star: star.rgb(),
            dot_red: dot_red.rgb(),
            dot_yellow: dot_yellow.rgb(),
            dot_green: dot_green.rgb(),
        }
    }

    pub fn r_bg(self) -> RColor {
        rgb(self.bg)
    }

    pub fn r_surface(self) -> RColor {
        rgb(self.surface)
    }

    pub fn r_border(self) -> RColor {
        rgb(self.border)
    }

    pub fn r_fg(self) -> RColor {
        rgb(self.fg)
    }

    pub fn r_muted(self) -> RColor {
        rgb(self.muted)
    }

    pub fn r_accent(self) -> RColor {
        rgb(self.accent)
    }

    pub fn r_secondary(self) -> RColor {
        rgb(self.secondary)
    }

    pub fn r_tertiary(self) -> RColor {
        rgb(self.tertiary)
    }

    pub fn r_star(self) -> RColor {
        rgb(self.star)
    }

    pub fn r_green(self) -> RColor {
        rgb(self.dot_green)
    }

    pub fn r_yellow(self) -> RColor {
        rgb(self.dot_yellow)
    }

    pub fn r_red(self) -> RColor {
        rgb(self.dot_red)
    }
}

fn rgb(c: (u8, u8, u8)) -> RColor {
    RColor::Rgb(c.0, c.1, c.2)
}

/// 22×22 pixel matrix faithfully derived from `assets/logo.svg`:
/// - `B`: neon outer frame (`border`)
/// - `C`: top-left / bottom-left corner pixel (`secondary`)
/// - `P`: top-right / bottom-right corner & cursor `_` (`accent`)
/// - `T`: titlebar fill (`surface`)
/// - `R`, `Y`, `G`: traffic-light window dots (`dot_red`, `dot_yellow`, `dot_green`)
/// - `D`: midnight CRT window body (`bg`)
/// - `>`: glowing `>` prompt chevron (`secondary`)
/// - `S`: 4-point Kiseki star (`star`)
/// - `W`: star center highlight (`#FFFFFF`)
/// - `K`: companion sparkle (`tertiary`)
/// - `L`: subtle bottom CRT scanline bar (`surface`)
/// - `.`: transparent outside window
const FULL_LOGO_GRID: [&str; 22] = [
    ".CBBBBBBBBBBBBBBBBBBP.",
    "CTTTTTTTTTTTTTTTTTTTTP",
    "BTRDTYDTGDTTTTTTTTTTTB",
    "BTTTTTTTTTTTTTTTTTTTTB",
    "BBBBBBBBBBBBBBBBBBBBBB",
    "BDDDDDDDDDDDDDDDDDDDDB",
    "BDDDDDDDDDDDDDDDSDDDDB",
    "BDDDDDDDDDDPDDDSWSDDDB",
    "BDDD>>DDDDDDDDDSSSDDDB",
    "BDDDD>>DDDDDDDDDSDDDDB",
    "BDDDDD>>DDDDDDDDDDDDDB",
    "BDDDDDD>>DDDDDDDDDKDDB",
    "BDDDDD>>DDDDDDDDDDDDDB",
    "BDDDD>>DDDDDDDDDDDDDDB",
    "BDDD>>DDDddddddDDDDDDB",
    "BDDDDDDDDPPPPPPDDDKDDB",
    "BDDDDDDDDPPPPPPDDKKDDB",
    "BDDDDDDDDDDDDDDDDDDDDB",
    "BDDLLLLLLLLLLLLLLLLDDB",
    "BDDDDDDDDDDDDDDDDDDDDB",
    "CDDDDDDDDDDDDDDDDDDDDP",
    ".CBBBBBBBBBBBBBBBBBBP.",
];

/// Compact 12×10 pixel matrix (12 cols × 5 rows in half-blocks) for CLI banners.
const MINI_LOGO_GRID: [&str; 10] = [
    ".CBBBBBBBBP.",
    "BTRTYTGTTTTB",
    "BBBBBBBBBBBB",
    "BDD>DDDDSDDB",
    "BDDD>DDSWSDb",
    "BDD>DDDDSDDB",
    "BDDDDPPPDKDB",
    "BDDLLLLLLDDB",
    "BDDDDDDDDDDB",
    ".CBBBBBBBBP.",
];

fn pixel_color(ch: char, s: &ThemeSwatch) -> Option<(u8, u8, u8)> {
    match ch {
        '.' => None,
        'B' | 'b' => Some(s.border),
        'C' => Some(s.secondary),
        'P' => Some(s.accent),
        'T' | 'L' => Some(s.surface),
        'R' => Some(s.dot_red),
        'Y' => Some(s.dot_yellow),
        'G' => Some(s.dot_green),
        'D' | 'd' => Some(s.bg),
        '>' => Some(s.secondary),
        'S' => Some(s.star),
        'W' => Some((255, 255, 255)),
        'K' => Some(s.tertiary),
        _ => None,
    }
}

/// Renders the 22×11 TrueColor half-block Kiseki logo for Ratatui.
pub fn logo_ratatui_lines(swatch: &ThemeSwatch, ascii_only: bool) -> Vec<Line<'static>> {
    if ascii_only {
        return ascii_logo_ratatui_lines(swatch);
    }
    let mut lines = Vec::with_capacity(FULL_LOGO_GRID.len() / 2);
    for pair in FULL_LOGO_GRID.chunks_exact(2) {
        let top_chars: Vec<char> = pair[0].chars().collect();
        let bot_chars: Vec<char> = pair[1].chars().collect();
        let mut spans = Vec::with_capacity(top_chars.len());
        for (tc, bc) in top_chars.into_iter().zip(bot_chars) {
            let top = pixel_color(tc, swatch);
            let bot = pixel_color(bc, swatch);
            let span = match (top, bot) {
                (None, None) => Span::raw(" "),
                (Some(t), None) => Span::styled("▀", Style::default().fg(rgb(t))),
                (None, Some(b)) => Span::styled("▄", Style::default().fg(rgb(b))),
                (Some(t), Some(b)) if t == b => Span::styled("█", Style::default().fg(rgb(t))),
                (Some(t), Some(b)) => Span::styled("▀", Style::default().fg(rgb(t)).bg(rgb(b))),
            };
            spans.push(span);
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Pure-ASCII 22×9 fallback logo (100% 1-col ASCII, zero alignment drift).
pub fn ascii_logo_ratatui_lines(swatch: &ThemeSwatch) -> Vec<Line<'static>> {
    let border = Style::default().fg(swatch.r_border());
    let prompt = Style::default()
        .fg(swatch.r_secondary())
        .add_modifier(Modifier::BOLD);
    let cursor = Style::default()
        .fg(swatch.r_accent())
        .add_modifier(Modifier::BOLD);
    let star = Style::default()
        .fg(swatch.r_star())
        .add_modifier(Modifier::BOLD);
    let r = Style::default().fg(swatch.r_red());
    let y = Style::default().fg(swatch.r_yellow());
    let g = Style::default().fg(swatch.r_green());

    vec![
        Line::from(Span::styled("+--------------------+", border)),
        Line::from(vec![
            Span::styled("| ", border),
            Span::styled("o", r),
            Span::raw(" "),
            Span::styled("o", y),
            Span::raw(" "),
            Span::styled("o", g),
            Span::raw("              "),
            Span::styled("|", border),
        ]),
        Line::from(Span::styled("+--------------------+", border)),
        Line::from(vec![
            Span::styled("|", border),
            Span::raw("              "),
            Span::styled("*", star),
            Span::raw("     "),
            Span::styled("|", border),
        ]),
        Line::from(vec![
            Span::styled("|", border),
            Span::raw("   "),
            Span::styled("\\", prompt),
            Span::raw("         "),
            Span::styled("-*-", star),
            Span::raw("    "),
            Span::styled("|", border),
        ]),
        Line::from(vec![
            Span::styled("|", border),
            Span::raw("    "),
            Span::styled(">", prompt),
            Span::raw("         "),
            Span::styled("*", star),
            Span::raw("     "),
            Span::styled("|", border),
        ]),
        Line::from(vec![
            Span::styled("|", border),
            Span::raw("   "),
            Span::styled("/", prompt),
            Span::raw("   "),
            Span::styled("_____", cursor),
            Span::raw("        "),
            Span::styled("|", border),
        ]),
        Line::from(vec![
            Span::styled("|", border),
            Span::raw("                    "),
            Span::styled("|", border),
        ]),
        Line::from(Span::styled("+--------------------+", border)),
    ]
}

/// Renders the compact 12×5 Kiseki window logo as ANSI strings for CLI banners.
pub fn render_mini_cli_logo(swatch: &ThemeSwatch, color: bool) -> Vec<String> {
    if !color {
        return vec![
            "+----------+".to_string(),
            "| o o o    |".to_string(),
            "+----------+".to_string(),
            "|  > _   * |".to_string(),
            "+----------+".to_string(),
        ];
    }
    let mut out = Vec::with_capacity(MINI_LOGO_GRID.len() / 2);
    for pair in MINI_LOGO_GRID.chunks_exact(2) {
        let mut line = String::new();
        for (tc, bc) in pair[0].chars().zip(pair[1].chars()) {
            let top = pixel_color(tc, swatch);
            let bot = pixel_color(bc, swatch);
            match (top, bot) {
                (None, None) => line.push(' '),
                (Some((r, g, b)), None) => {
                    line.push_str(&format!("\x1b[38;2;{r};{g};{b}m▀\x1b[0m"));
                }
                (None, Some((r, g, b))) => {
                    line.push_str(&format!("\x1b[38;2;{r};{g};{b}m▄\x1b[0m"));
                }
                (Some(t), Some(b)) if t == b => {
                    line.push_str(&format!("\x1b[38;2;{};{};{}m█\x1b[0m", t.0, t.1, t.2));
                }
                (Some((tr, tg, tb)), Some((br, bg, bb))) => {
                    line.push_str(&format!(
                        "\x1b[38;2;{tr};{tg};{tb};48;2;{br};{bg};{bb}m▀\x1b[0m"
                    ));
                }
            }
        }
        out.push(line);
    }
    out
}

/// True when the terminal likely lacks TrueColor or `NO_COLOR` is active.
pub fn prefer_ascii_logo() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return true;
    }
    if std::env::var("TERM").is_ok_and(|term| term == "dumb") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_logo_grid_rows_have_uniform_width() {
        for row in FULL_LOGO_GRID {
            assert_eq!(row.chars().count(), 22);
        }
    }

    #[test]
    fn mini_logo_grid_rows_have_uniform_width() {
        for row in MINI_LOGO_GRID {
            assert_eq!(row.chars().count(), 12);
        }
    }

    #[test]
    fn renders_both_halfblock_and_ascii_variants() {
        let swatch = ThemeSwatch::default();
        assert_eq!(logo_ratatui_lines(&swatch, false).len(), 11);
        assert_eq!(logo_ratatui_lines(&swatch, true).len(), 9);
        assert_eq!(render_mini_cli_logo(&swatch, true).len(), 5);
        assert_eq!(render_mini_cli_logo(&swatch, false).len(), 5);
    }
}
