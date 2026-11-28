//! CLI `--help` / `--version` flag localization.

use clap::{Arg, ArgAction, Command};

/// Display language for CLI help metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// Detects the active CLI help language.
///
/// Defaults to [`Lang::Zh`] to match Kiseki's Chinese CLI documentation, while
/// honouring an explicit `KISEKI_LANG=en` override.
pub fn detect_lang() -> Lang {
    if let Ok(value) = std::env::var("KISEKI_LANG") {
        let lower = value.trim().to_ascii_lowercase();
        if lower.starts_with("en") {
            return Lang::En;
        }
        if lower.starts_with("zh") {
            return Lang::Zh;
        }
    }
    Lang::Zh
}

/// Applies help localization recursively to `cmd` and all nested subcommands.
///
/// Under [`Lang::Zh`], replaces clap's auto-generated `-h, --help`
/// (`Print help`) and `-V, --version` (`Print version`) flags with Chinese
/// descriptions so no English boilerplate leaks into Chinese `--help` output.
pub fn localize_command(cmd: Command, lang: Lang) -> Command {
    localize_inner(cmd, lang, true)
}

fn localize_inner(mut cmd: Command, lang: Lang, is_root: bool) -> Command {
    if matches!(lang, Lang::Zh) {
        cmd = cmd.disable_help_flag(true).arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("打印帮助信息"),
        );
        if is_root && cmd.get_version().is_some() {
            cmd = cmd.disable_version_flag(true).arg(
                Arg::new("version")
                    .short('V')
                    .long("version")
                    .action(ArgAction::Version)
                    .help("打印版本信息"),
            );
        }
        cmd = cmd.mut_subcommands(|sub| localize_inner(sub, lang, false));
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn zh_localizes_root_and_subcommand_help_and_version_flags() {
        let mut root = localize_command(crate::Cli::command(), Lang::Zh);
        let root_help = root.render_long_help().to_string();
        assert!(root_help.contains("打印帮助信息"), "{root_help}");
        assert!(root_help.contains("打印版本信息"), "{root_help}");
        assert!(!root_help.contains("Print help"), "{root_help}");
        assert!(!root_help.contains("Print version"), "{root_help}");

        for sub in root.get_subcommands_mut() {
            let sub_help = sub.render_long_help().to_string();
            assert!(
                sub_help.contains("打印帮助信息"),
                "subcommand `{}` missing Chinese help flag:\n{sub_help}",
                sub.get_name()
            );
            assert!(
                !sub_help.contains("Print help"),
                "subcommand `{}` still contains English `Print help`:\n{sub_help}",
                sub.get_name()
            );
        }
    }

    #[test]
    fn en_preserves_clap_default_help_and_version_flags() {
        let mut root = localize_command(crate::Cli::command(), Lang::En);
        let root_help = root.render_long_help().to_string();
        assert!(root_help.contains("Print help"), "{root_help}");
        assert!(root_help.contains("Print version"), "{root_help}");
    }
}
