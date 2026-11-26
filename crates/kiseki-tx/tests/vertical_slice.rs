//! End-to-end vertical slice: compile → plan → journal → apply → rollback.
//!
//! Uses `MockPlatform` over a real temporary filesystem so symlink, permission
//! and atomic-rename behaviour is exercised exactly as it is on a user machine.

use std::path::{Path, PathBuf};

use kiseki_adapters::AdapterRegistry;
use kiseki_compiler::{compile, CompileInput, CompileOptions};
use kiseki_platform::{MockPlatform, PathKind, Platform};
use kiseki_schema::{ThemePack, ResolvedTheme};
use kiseki_state::{Config, PreviewRecovery, PreviewSession, State};
use kiseki_tx::{ApplyRequest, TransactionManager, TransactionOptions};
use kiseki_types::{EnvContext, PromptPolicy, RenderingProfile, ThemeId};

const GLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.glsl");
const HLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.hlsl");
const LOGO: &str = include_str!("../../../themes/nerv-eva/assets/logo.txt");

struct Host {
    _dir: tempfile::TempDir,
    platform: MockPlatform,
    zshrc: PathBuf,
    ghostty_config: PathBuf,
    wt_settings: PathBuf,
}

fn theme() -> ResolvedTheme {
    let pack = ThemePack {
        manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
        tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
    };
    ResolvedTheme::from_pack(&pack, RenderingProfile::Balanced)
}

fn compile_for(config_dir: &Path) -> kiseki_compiler::CompileOutput {
    let theme = theme();
    let options = CompileOptions::new(config_dir, "/themes/nerv-eva")
        .with_prompt_policy(PromptPolicy::ManagedTemplate)
        .with_font_families(vec!["JetBrainsMono Nerd Font".to_string()]);
    compile(
        &CompileInput::new(&theme, options)
            .with_glsl(Some(GLSL))
            .with_hlsl(Some(HLSL))
            .with_logo(Some(LOGO)),
    )
    .unwrap()
}

fn host() -> Host {
    let dir = tempfile::tempdir().unwrap();
    let platform = MockPlatform::new(dir.path());
    let zshrc = dir.path().join("dotfiles").join(".zshrc");
    let ghostty_config = dir.path().join("ghostty").join("config");
    let wt_settings = dir.path().join("WindowsTerminal").join("settings.json");
    for path in [&zshrc, &ghostty_config, &wt_settings] {
        platform.create_dir_all(path.parent().unwrap()).unwrap();
    }
    platform
        .write_preserving_symlinks(
            &zshrc,
            b"# user zshrc\nexport EDITOR=nvim\nalias ll='ls -l'\n",
        )
        .unwrap();
    platform
        .write_preserving_symlinks(&ghostty_config, b"# user ghostty\nfont-size = 12\n")
        .unwrap();
    platform
        .write_preserving_symlinks(
            &wt_settings,
            br##"{
  // user comment that must survive
  "profiles": {
    "defaults": { "font": { "face": "Cascadia Code", "size": 12 } },
    "list": [
      { "name": "PowerShell", "colorScheme": "One Half Dark" } // pinned
    ]
  },
  "schemes": [],
  "keybindings": [{ "keys": "ctrl+c", "command": "copy" }]
}
"##,
        )
        .unwrap();
    platform.set_env("KISEKI_ZSHRC", &zshrc.to_string_lossy());
    platform.set_env("KISEKI_GHOSTTY_CONFIG", &ghostty_config.to_string_lossy());
    platform.set_env("KISEKI_PWSH_PROFILE", &dir.path().join("profile.ps1").to_string_lossy());
    platform.set_env("KISEKI_WT_SETTINGS", &wt_settings.to_string_lossy());
    Host {
        _dir: dir,
        platform,
        zshrc,
        ghostty_config,
        wt_settings,
    }
}

fn state(platform: &MockPlatform) -> State {
    State::new(
        ThemeId::new("nerv-eva").unwrap(),
        &platform.now_rfc3339(),
    )
}

#[test]
fn full_transaction_touches_every_host_and_is_reversible() {
    let host = host();
    let paths = host.platform.paths();
    let output = compile_for(paths.config_dir());
    let theme = theme();
    let env = EnvContext::local();
    let before = state(&host.platform);
    let mut after = before.clone();
    after.rendering.profile = RenderingProfile::Ultra;
    after.rendering.crt = true;

    let manager = TransactionManager::new(&host.platform, AdapterRegistry::milestone1());
    let report = manager
        .apply(ApplyRequest {
            theme: &theme,
            artifacts: &output.artifacts,
            state_before: before.clone(),
            state_after: after.clone(),
            env: &env,
            options: TransactionOptions {
                command: "use nerv-eva --profile ultra".to_string(),
                ..TransactionOptions::default()
            },
        })
        .unwrap();

    assert!(!report.has_failures(), "{:?}", report.targets);
    assert_eq!(report.state.rendering.profile, RenderingProfile::Ultra);
    assert!(report.state.rendering.crt);

    // --- generated artefacts --------------------------------------------
    for artifact in [
        "ghostty.conf",
        "env.zsh",
        "env.ps1",
        "prompt.omp.json",
        "starship-palette.toml",
        "fastfetch.jsonc",
        "wt-visual.json",
        "shaders/active.glsl",
        "shaders/active.hlsl",
    ] {
        assert!(
            host.platform.exists(&paths.generated(artifact)),
            "missing generated/{artifact}"
        );
    }
    let env_zsh = host
        .platform
        .read_to_string(&paths.generated("env.zsh"))
        .unwrap()
        .unwrap();
    assert!(!env_zsh.contains("alias cat"));
    assert!(!env_zsh.contains("zoxide"));
    let prompt = host
        .platform
        .read_to_string(&paths.generated("prompt.omp.json"))
        .unwrap()
        .unwrap();
    assert!(!prompt.contains("\"command\""));
    let fastfetch = host
        .platform
        .read_to_string(&paths.generated("fastfetch.jsonc"))
        .unwrap()
        .unwrap();
    assert!(!fastfetch.contains("\"command\""));
    assert!(fastfetch.contains(&paths.official_themes_dir().join("nerv-eva").to_string_lossy().to_string())
        || fastfetch.contains("/themes/nerv-eva/assets/logo.txt"));

    // --- zsh anchor ------------------------------------------------------
    let zshrc = host.platform.read_to_string(&host.zshrc).unwrap().unwrap();
    assert!(zshrc.contains("# user zshrc"));
    assert!(zshrc.contains("alias ll='ls -l'"));
    assert!(zshrc.contains("# >>> kiseki >>>"));
    assert!(zshrc.contains("add-zsh-hook precmd _kiseki_sync_hook"));
    assert!(!zshrc.contains("alias cat="));
    assert!(!zshrc.contains("zoxide"));
    assert!(!zshrc.contains("bindkey"));

    // --- Ghostty include -------------------------------------------------
    let ghostty = host
        .platform
        .read_to_string(&host.ghostty_config)
        .unwrap()
        .unwrap();
    assert!(ghostty.contains("# user ghostty"));
    assert!(ghostty.contains(&format!(
        "config-file = ?{}",
        paths.generated("ghostty.conf").display()
    )));
    let ghostty_generated = host
        .platform
        .read_to_string(&paths.generated("ghostty.conf"))
        .unwrap()
        .unwrap();
    assert!(ghostty_generated.contains("custom-shader = "));
    assert!(!ghostty_generated.contains("shell-integration"));

    // --- Windows Terminal JSONC patch ------------------------------------
    let settings = host.platform.read_to_string(&host.wt_settings).unwrap().unwrap();
    assert!(settings.contains("// user comment that must survive"));
    assert!(settings.contains("// pinned"));
    assert!(settings.contains("\"JetBrainsMono Nerd Font\""));
    assert!(settings.contains("\"keys\": \"ctrl+c\""));
    assert!(settings.contains("\"colorScheme\": \"Kiseki: NERV UNIT-02\""));
    assert!(!settings.contains("scrollbarState"));
    assert!(!settings.contains("bellStyle"));
    let baseline = paths.journal_dir().join("wt-baseline.json");
    assert!(host.platform.exists(&baseline));

    let gen = State::read_gen(&host.platform, paths).unwrap().unwrap();
    assert_eq!(gen, report.state.generation);

    // --- rollback --------------------------------------------------------
    let rollback = manager.rollback(&report.tx_id).unwrap();
    // Content is restored, but the generation stays strictly monotonic so a
    // shell that cached an old `state.gen` always reloads `env.*`.
    assert_eq!(rollback.state.active_theme, before.active_theme);
    assert_eq!(rollback.state.rendering, before.rendering);
    assert!(rollback.state.generation > report.state.generation);
    assert_eq!(
        host.platform.read_to_string(&host.zshrc).unwrap().unwrap(),
        "# user zshrc\nexport EDITOR=nvim\nalias ll='ls -l'\n"
    );
    let settings_after = host.platform.read_to_string(&host.wt_settings).unwrap().unwrap();
    assert!(settings_after.contains("// user comment that must survive"));
    assert!(!settings_after.contains("Kiseki: NERV UNIT-02"));
    assert!(settings_after.contains("One Half Dark"));
    assert_eq!(host.platform.read_to_string(&host.ghostty_config).unwrap().unwrap(), "# user ghostty\nfont-size = 12\n");
    assert_eq!(
        State::read_gen(&host.platform, paths).unwrap(),
        Some(rollback.state.generation)
    );
    // Generated artefacts are backed up and restored/removed by rollback.
    assert!(!host.platform.exists(&paths.generated("ghostty.conf")));
}

#[test]
fn read_only_dotfile_downgrades_but_keeps_generated_output() {
    let host = host();
    // Replace the zshrc with a read-only symlink (Nix/Home Manager scenario).
    let target = host._dir.path().join("nix-store").join("zshrc");
    host.platform.create_dir_all(target.parent().unwrap()).unwrap();
    host.platform.write_preserving_symlinks(&target, b"# nix managed\n").unwrap();
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&target, permissions).unwrap();
    std::fs::remove_file(&host.zshrc).unwrap();
    symlink(&target, &host.zshrc);
    assert!(matches!(
        host.platform.inspect(&host.zshrc),
        PathKind::ReadOnlySymlink(_)
    ));

    let output = compile_for(host.platform.paths().config_dir());
    let theme = theme();
    let env = EnvContext::local();
    let before = state(&host.platform);
    let manager = TransactionManager::new(&host.platform, AdapterRegistry::milestone1());
    let report = manager
        .apply(ApplyRequest {
            theme: &theme,
            artifacts: &output.artifacts,
            state_before: before.clone(),
            state_after: before,
            env: &env,
            options: TransactionOptions::default(),
        })
        .unwrap();

    assert!(!report.has_failures());
    assert!(report
        .targets
        .iter()
        .any(|target| target.adapter == "zsh" && target.status == kiseki_tx::TargetStatus::RenderOnly));
    assert!(std::fs::symlink_metadata(&host.zshrc)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(host.platform.read_to_string(&target).unwrap().unwrap(), "# nix managed\n");
    assert!(host
        .platform
        .exists(&host.platform.paths().generated("env.zsh")));
    // The render-only hint is copy-pasteable.
    assert!(report.diagnostics.iter().any(|diagnostic| diagnostic
        .hint
        .as_deref()
        .is_some_and(|hint| hint.contains("env.zsh"))));
}

#[test]
fn preview_crash_is_recovered_before_the_next_transaction() {
    let host = host();
    let paths = host.platform.paths();
    let before = state(&host.platform);
    before.save(&host.platform, paths).unwrap();
    before.write_gen(&host.platform, paths).unwrap();

    // Simulate a picker that previewed a theme and was SIGKILLed.
    let recovery = PreviewRecovery {
        schema_version: 1,
        pid: 4_242,
        started_at: host.platform.now_rfc3339(),
        preview_theme: ThemeId::new("kyoto-sakura").unwrap(),
        previous_state: before.clone(),
    };
    recovery.save(&host.platform, paths).unwrap();
    host.platform.set_pid_alive(4_242, false);
    assert!(PreviewSession::is_active(&host.platform, paths).unwrap());
    let recovered = PreviewSession::recover_stale(&host.platform, paths)
        .unwrap()
        .expect("stale preview recovered");
    assert_eq!(recovered.previous_state, before);
    assert!(!PreviewSession::is_active(&host.platform, paths).unwrap());
    assert_eq!(
        State::load(&host.platform, paths).unwrap().unwrap(),
        before
    );
}

#[test]
fn config_is_round_tripped_through_the_real_toml_file() {
    let host = host();
    let paths = host.platform.paths();
    let config = Config {
        prompt_policy: PromptPolicy::PaletteOnly,
        render_only: true,
        shell_aliases: true,
        ..Config::default()
    };
    config.save(&host.platform, paths).unwrap();
    assert_eq!(Config::load(&host.platform, paths).unwrap(), config);
    let raw = host.platform.read_to_string(&paths.config_file()).unwrap().unwrap();
    assert!(raw.contains("palette-only"));
}

fn symlink(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).unwrap();
}
