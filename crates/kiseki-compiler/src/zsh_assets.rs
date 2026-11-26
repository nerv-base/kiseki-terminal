//! Pinned Zsh plugin assets installed into Kiseki's private vendor directory.

macro_rules! asset {
    ($path:literal) => {
        ($path, include_str!(concat!("../assets/zsh/", $path)))
    };
}

/// Paths are relative to `vendor/share/zsh`. Entrypoints come last so a
/// fresh installation never exposes a sourceable script before its helpers.
pub const ASSETS: &[(&str, &str)] = &[
    asset!("NOTICE"),
    (
        "LICENSE.zsh-syntax-highlighting",
        include_str!("../assets/zsh/zsh-syntax-highlighting/LICENSE"),
    ),
    (
        "LICENSE.zsh-autosuggestions",
        include_str!("../assets/zsh/zsh-autosuggestions/LICENSE"),
    ),
    asset!("zsh-syntax-highlighting/.version"),
    asset!("zsh-syntax-highlighting/.revision-hash"),
    asset!("zsh-syntax-highlighting/highlighters/brackets/brackets-highlighter.zsh"),
    asset!("zsh-syntax-highlighting/highlighters/cursor/cursor-highlighter.zsh"),
    asset!("zsh-syntax-highlighting/highlighters/line/line-highlighter.zsh"),
    asset!("zsh-syntax-highlighting/highlighters/main/main-highlighter.zsh"),
    asset!("zsh-syntax-highlighting/highlighters/pattern/pattern-highlighter.zsh"),
    asset!("zsh-syntax-highlighting/highlighters/regexp/regexp-highlighter.zsh"),
    asset!("zsh-syntax-highlighting/highlighters/root/root-highlighter.zsh"),
    asset!("zsh-autosuggestions/src/async.zsh"),
    asset!("zsh-autosuggestions/src/bind.zsh"),
    asset!("zsh-autosuggestions/src/config.zsh"),
    asset!("zsh-autosuggestions/src/fetch.zsh"),
    asset!("zsh-autosuggestions/src/highlight.zsh"),
    asset!("zsh-autosuggestions/src/start.zsh"),
    asset!("zsh-autosuggestions/src/strategies/completion.zsh"),
    asset!("zsh-autosuggestions/src/strategies/history.zsh"),
    asset!("zsh-autosuggestions/src/strategies/match_prev_cmd.zsh"),
    asset!("zsh-autosuggestions/src/util.zsh"),
    asset!("zsh-autosuggestions/src/widgets.zsh"),
    asset!("zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"),
    asset!("zsh-autosuggestions/zsh-autosuggestions.zsh"),
];
