//! CLI integration tests.
//!
//! Every test runs the real `kiseki` binary against a throwaway `KISEKI_HOME`
//! and throwaway host config files, so the developer's dotfiles are never
//! touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".config")).unwrap();
        fs::write(home.join(".zshrc"), "# user zshrc\nalias ll='ls -l'\n").unwrap();
        fs::write(home.join("ghostty-config"), "# user ghostty\nfont-size = 12\n").unwrap();
        fs::write(
            home.join("wt-settings.json"),
            "{\n  // keep this comment\n  \"profiles\": {\n    \"defaults\": {},\n    \"list\": [\n      { \"name\": \"PowerShell\", \"colorScheme\": \"One Half Dark\" }\n    ]\n  },\n  \"schemes\": [],\n  \"keybindings\": [{ \"keys\": \"ctrl+c\", \"command\": \"copy\" }]\n}\n",
        )
        .unwrap();
        Self { dir }
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn kiseki_home(&self) -> PathBuf {
        self.dir.path().join("kiseki")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.home().join(name)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_kiseki"));
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.home())
            .env("KISEKI_HOME", self.kiseki_home())
            .env("KISEKI_ZSHRC", self.path(".zshrc"))
            .env("KISEKI_GHOSTTY_CONFIG", self.path("ghostty-config"))
            .env("KISEKI_PWSH_PROFILE", self.path("profile.ps1"))
            .env("KISEKI_WT_SETTINGS", self.path("wt-settings.json"))
            .env("NO_COLOR", "1")
            .env("KISEKI_APPEARANCE", "dark")
            .env("KISEKI_POWER_SOURCE", "ac");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command()
            .args(args)
            .output()
            .expect("kiseki binary runs")
    }

    fn run_ok(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "kiseki {args:?} failed ({}):\nstdout={}\nstderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run_ok(args);
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "kiseki {args:?} did not emit JSON: {error}\n{}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }
}

#[test]
fn version_and_help_are_available() {
    let sandbox = Sandbox::new();
    let output = sandbox.run_ok(&["--version"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kiseki 2.0.0"), "{stdout}");

    let output = sandbox.run_ok(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for command in ["use", "pick", "status", "compose", "profile", "doctor", "theme", "shell-init"] {
        assert!(stdout.contains(command), "help is missing {command}:\n{stdout}");
    }
}

#[test]
fn theme_list_works_before_initialisation() {
    let sandbox = Sandbox::new();
    let json = sandbox.json(&["theme", "list", "--json"]);
    let themes = json["themes"].as_array().unwrap();
    assert_eq!(themes.len(), 4);
    let ids: Vec<&str> = themes
        .iter()
        .filter_map(|t| t["id"].as_str())
        .collect();
    for expected in ["nerv-eva", "kyoto-sakura", "shinkai-rain", "starlight-8bit"] {
        assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
    }
    assert!(themes.iter().all(|t| t["source"] == "bundled"));
}

#[test]
fn render_only_use_compiles_every_artifact_without_touching_hosts() {
    let sandbox = Sandbox::new();
    let zshrc_before = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    let ghostty_before = fs::read_to_string(sandbox.path("ghostty-config")).unwrap();
    let wt_before = fs::read_to_string(sandbox.path("wt-settings.json")).unwrap();

    let json = sandbox.json(&["use", "nerv-eva", "--render-only", "--json"]);
    assert_eq!(json["dryRun"], false);
    assert_eq!(json["renderOnly"], true);
    assert!(json["generation"].as_u64().unwrap() >= 2);

    for artifact in [
        "generated/ghostty.conf",
        "generated/env.zsh",
        "generated/env.ps1",
        "generated/prompt.omp.json",
        "generated/starship-palette.toml",
        "generated/fastfetch.jsonc",
        "generated/wt-visual.json",
        "generated/shaders/active.glsl",
        "generated/shaders/active.hlsl",
        "state.json",
        "state.gen",
    ] {
        assert!(
            sandbox.kiseki_home().join(artifact).exists(),
            "missing {artifact}"
        );
    }
    assert_eq!(fs::read_to_string(sandbox.path(".zshrc")).unwrap(), zshrc_before);
    assert_eq!(fs::read_to_string(sandbox.path("ghostty-config")).unwrap(), ghostty_before);
    assert_eq!(fs::read_to_string(sandbox.path("wt-settings.json")).unwrap(), wt_before);

    let status = sandbox.json(&["status", "--json"]);
    assert_eq!(status["state"]["activeTheme"], "nerv-eva");
    assert_eq!(status["theme"]["id"], "nerv-eva");
    assert!(status["artifacts"].as_array().unwrap().iter().any(|value| value == "shaders/active.glsl"));
}

#[test]
fn full_use_mounts_hosts_and_rollback_restores_them() {
    let sandbox = Sandbox::new();
    let json = sandbox.json(&["use", "nerv-eva", "--json"]);
    assert!(json["targets"].as_array().unwrap().iter().any(|target| target["status"] == "applied"));

    let zshrc = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    assert!(zshrc.contains("# user zshrc"));
    assert!(zshrc.contains("# >>> kiseki >>>"));
    assert!(zshrc.contains("add-zsh-hook precmd _kiseki_sync_hook"));
    assert!(!zshrc.contains("alias cat="));

    let ghostty = fs::read_to_string(sandbox.path("ghostty-config")).unwrap();
    assert!(ghostty.contains("# user ghostty"));
    assert!(ghostty.contains("config-file = ?"));

    let wt = fs::read_to_string(sandbox.path("wt-settings.json")).unwrap();
    assert!(wt.contains("// keep this comment"));
    assert!(wt.contains("\"colorScheme\": \"Kiseki: NERV UNIT-02\""));
    assert!(wt.contains("\"keys\": \"ctrl+c\""));

    let journals = sandbox.json(&["rollback", "--list", "--json"]);
    assert_eq!(journals["journals"].as_array().unwrap().len(), 1);
    let output = sandbox.run_ok(&["rollback", "--quiet"]);
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(sandbox.path(".zshrc")).unwrap(),
        "# user zshrc\nalias ll='ls -l'\n"
    );
    assert_eq!(
        fs::read_to_string(sandbox.path("ghostty-config")).unwrap(),
        "# user ghostty\nfont-size = 12\n"
    );
    let wt = fs::read_to_string(sandbox.path("wt-settings.json")).unwrap();
    assert!(wt.contains("// keep this comment"));
    assert!(wt.contains("One Half Dark"));
    assert!(!wt.contains("Kiseki: NERV UNIT-02"));
}

#[test]
fn theme_validate_passes_for_the_official_pack() {
    let sandbox = Sandbox::new();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/nerv-eva");
    let json = sandbox.json(&["theme", "validate", path.to_str().unwrap(), "--json"]);
    assert_eq!(json["valid"], true);
    assert_eq!(json["files"], 9);
    let shaders = json["shaders"].as_array().unwrap();
    assert_eq!(shaders.len(), 6);
    assert!(shaders.iter().all(|summary| summary.as_str().unwrap().contains("ok")));
}

#[test]
fn invalid_theme_is_rejected_with_a_nonzero_exit() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["use", "not-a-theme"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not-a-theme"), "{stderr}");
}

#[test]
fn doctor_reports_after_use_and_fix_is_safe() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    let json = sandbox.json(&["doctor", "--json"]);
    let checks = json["checks"].as_array().unwrap();
    assert!(!checks.is_empty());
    assert!(checks.iter().any(|check| check["name"] == "state.active"));
    assert!(checks.iter().any(|check| check["name"] == "adapter.zsh.detect"));
    let output = sandbox.run_ok(&["doctor", "--fix", "--quiet"]);
    assert!(output.status.success());
}

#[test]
fn shell_init_is_zero_fork_and_completions_are_generated() {
    let sandbox = Sandbox::new();
    let output = sandbox.run_ok(&["shell-init", "zsh"]);
    let script = String::from_utf8_lossy(&output.stdout);
    assert!(script.contains("_kiseki_sync_hook"));
    assert!(script.contains("state.gen"));
    assert!(!script.contains("$("));

    let output = sandbox.run_ok(&["shell-init", "pwsh"]);
    let script = String::from_utf8_lossy(&output.stdout);
    assert!(script.contains("[System.IO.File]::ReadAllText"));

    let output = sandbox.run_ok(&["completions", "zsh"]);
    let script = String::from_utf8_lossy(&output.stdout);
    assert!(script.starts_with("#compdef kiseki"), "{script}");

    let output = sandbox.run_ok(&["completions", "bash"]);
    assert!(!output.stdout.is_empty());
}

#[test]
fn recover_safe_forces_static_profile() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--profile", "ultra", "--render-only", "--quiet"]);
    let json = sandbox.json(&["recover", "--safe", "--json"]);
    assert_eq!(json["state"]["rendering"]["profile"], "static");
    let status = sandbox.json(&["status", "--json"]);
    assert_eq!(status["state"]["rendering"]["profile"], "static");
    let state: Value = serde_json::from_str(
        &fs::read_to_string(sandbox.kiseki_home().join("state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["rendering"]["profile"], "static");
}

#[test]
fn profile_live_crt_bg_and_font_round_trip_through_state() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    sandbox.run_ok(&["profile", "ultra", "--render-only", "--quiet"]);
    assert_eq!(
        sandbox.json(&["profile", "--json"])["profile"],
        "ultra"
    );
    sandbox.run_ok(&["crt", "on", "--render-only", "--quiet"]);
    sandbox.run_ok(&["bg", "0.55", "--render-only", "--quiet"]);
    sandbox.run_ok(&["font", "JetBrainsMono Nerd Font", "14", "--render-only", "--quiet"]);
    let state = sandbox.json(&["status", "--json"]);
    assert_eq!(state["state"]["rendering"]["profile"], "ultra");
    assert_eq!(state["state"]["rendering"]["crt"], true);
    assert_eq!(state["state"]["rendering"]["bgOpacityOverride"], 0.55);
    assert_eq!(
        state["state"]["typography"]["sizeOverridePt"],
        14.0
    );
}

#[test]
fn compose_and_reset_mix_layers() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    sandbox.run_ok(&["compose", "--atmosphere", "nerv-eva", "--palette", "nerv-eva", "--render-only", "--quiet"]);
    let state = sandbox.json(&["status", "--json"]);
    assert_eq!(state["state"]["composition"]["atmosphere"], "nerv-eva");
    assert_eq!(state["state"]["composition"]["palette"], "nerv-eva");
    sandbox.run_ok(&["compose", "--reset", "--render-only", "--quiet"]);
    let state = sandbox.json(&["status", "--json"]);
    assert!(state["state"]["composition"]["palette"].is_null());
}

#[test]
fn init_non_interactive_is_safe_and_idempotent() {
    let sandbox = Sandbox::new();
    let output = sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    assert!(output.status.success());
    let config = fs::read_to_string(sandbox.kiseki_home().join("config.toml")).unwrap();
    assert!(config.contains("managed-template"));
    let state = sandbox.json(&["status", "--json"]);
    assert_eq!(state["state"]["activeTheme"], "nerv-eva");
    // Re-running init must not fail or duplicate anchors.
    sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    let zshrc = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    assert_eq!(zshrc.matches("# >>> kiseki >>>").count(), 1);
}

#[test]
fn dry_run_reports_but_writes_nothing() {
    let sandbox = Sandbox::new();
    let output = sandbox.json(&["use", "nerv-eva", "--dry-run", "--json"]);
    assert_eq!(output["dryRun"], true);
    assert!(!sandbox.kiseki_home().join("state.json").exists());
    assert!(!sandbox.kiseki_home().join("generated").exists());
    assert!(!sandbox.kiseki_home().join("data").join("journal").exists());
    // No theme materialisation either: the store is not even created.
    assert!(!sandbox.kiseki_home().join("data").join("themes").exists());
    assert!(!sandbox
        .kiseki_home()
        .join("data/themes/official/.versions.json")
        .exists());
    // Not even the KISEKI_HOME root (created by the transaction lock) appears.
    assert!(!sandbox.kiseki_home().exists());
}

#[test]
fn pick_is_safe_without_a_tty() {
    let sandbox = Sandbox::new();
    let json = sandbox.json(&["pick", "--json"]);
    assert_eq!(json["cancelled"], true);
    assert!(!sandbox.kiseki_home().join("state.json").exists());
}

#[test]
fn only_filter_limits_host_writes() {
    let sandbox = Sandbox::new();
    let zshrc_before = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    sandbox.run_ok(&["use", "nerv-eva", "--only", "ghostty", "--quiet"]);
    assert_eq!(fs::read_to_string(sandbox.path(".zshrc")).unwrap(), zshrc_before);
    let ghostty = fs::read_to_string(sandbox.path("ghostty-config")).unwrap();
    assert!(ghostty.contains("# >>> kiseki >>>"));
}

#[test]
fn migrate_detects_plans_and_applies_a_v1_installation() {
    let sandbox = Sandbox::new();
    let legacy = sandbox.home().join(".config").join("kiseki-terminal");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("current-theme.txt"), "nerv-eva\n").unwrap();
    fs::write(legacy.join("live-mode.txt"), "on\n").unwrap();
    fs::write(legacy.join("crt-mode.txt"), "on\n").unwrap();
    fs::write(legacy.join("bg-opacity.txt"), "0.42\n").unwrap();
    let shim = sandbox.home().join("bin").join("theme");
    fs::create_dir_all(shim.parent().unwrap()).unwrap();
    fs::write(&shim, "#!/bin/sh\n# kiseki-terminal-shim\nexec kiseki use \"$@\"\n").unwrap();
    fs::write(
        legacy.join("installed-shims.txt"),
        format!("{}\n", shim.display()),
    )
    .unwrap();
    fs::write(
        sandbox.path(".zshrc"),
        "# user\n# >>> kiseki-terminal >>>\nsource ~/.kiseki-terminal/env.zsh\n# <<< kiseki-terminal <<<\n",
    )
    .unwrap();
    fs::write(legacy.join("env.zsh"), "export OLD=1\n").unwrap();

    let plan = sandbox.json(&["migrate", "--json"]);
    assert_eq!(plan["applied"], false);
    assert!(plan["plan"].as_array().unwrap().iter().any(|line| line
        .as_str()
        .unwrap()
        .contains("remove v1 hooks")));

    let applied = sandbox.json(&["migrate", "--apply", "--json"]);
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["state"]["activeTheme"], "nerv-eva");
    assert_eq!(applied["state"]["rendering"]["crt"], true);
    assert_eq!(applied["state"]["rendering"]["bgOpacityOverride"], 0.42);
    let zshrc = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    assert!(zshrc.contains("# user"));
    assert!(!zshrc.contains("kiseki-terminal"));
    assert!(!shim.exists());
    let backup = applied["backup"].as_str().unwrap();
    assert!(Path::new(backup).join("legacy").join("current-theme.txt").exists());
}

#[test]
fn uninstall_removes_hooks_and_optionally_purges() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    assert!(fs::read_to_string(sandbox.path(".zshrc"))
        .unwrap()
        .contains("# >>> kiseki >>>"));
    sandbox.run_ok(&["uninstall", "--json"]);
    let zshrc = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    assert!(!zshrc.contains("# >>> kiseki >>>"));
    assert!(!sandbox.kiseki_home().join("state.json").exists());
    assert!(!sandbox.kiseki_home().join("generated").exists());
    // Themes and journal survive a normal uninstall.
    assert!(sandbox
        .kiseki_home()
        .join("data")
        .join("themes")
        .join("official")
        .join("nerv-eva")
        .exists());

    sandbox.run_ok(&["uninstall", "--purge", "--quiet"]);
    assert!(!sandbox.kiseki_home().join("data").exists());
    assert!(!sandbox.kiseki_home().join("cache").exists());
}

#[test]
fn community_theme_install_info_and_remove() {
    let sandbox = Sandbox::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/nerv-eva");
    let installed = sandbox.json(&[
        "theme",
        "install",
        source.to_str().unwrap(),
        "--force",
        "--json",
    ]);
    assert_eq!(installed["id"], "nerv-eva");
    assert!(installed["files"].as_u64().unwrap() >= 9);

    let list = sandbox.json(&["theme", "list", "--json"]);
    let entry = list["themes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "nerv-eva")
        .unwrap();
    assert_eq!(entry["source"], "community");

    let info = sandbox.json(&["theme", "info", "nerv-eva", "--json"]);
    assert_eq!(info["manifest"]["id"], "nerv-eva");
    assert!(info["contrast"].as_array().unwrap().iter().any(|check| check["passes"] == true));

    sandbox.run_ok(&["theme", "remove", "nerv-eva", "--json"]);
    let list = sandbox.json(&["theme", "list", "--json"]);
    let entry = list["themes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "nerv-eva")
        .unwrap();
    assert_eq!(entry["source"], "bundled");
}

#[test]
fn fonts_status_reports_catalog_resolution() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    let json = sandbox.json(&["fonts", "status", "--json"]);
    let families = json["families"].as_array().unwrap();
    assert_eq!(families.len(), 2);
    assert_eq!(families[0]["requested"], "jetbrains-mono-nf");
    assert_eq!(families[0]["catalogId"], "jetbrains-mono-nf");
    assert!(families[0]["resolvedFamily"].as_str().unwrap().contains("JetBrainsMono"));
}

#[test]
fn shell_init_json_and_unsupported_shell_fail() {
    let sandbox = Sandbox::new();
    let json = sandbox.json(&["shell-init", "pwsh", "--json"]);
    assert!(json["script"].as_str().unwrap().contains("KisekiSyncHook"));
    let output = sandbox.run(&["shell-init", "fish"]);
    assert!(!output.status.success());
}

#[test]
fn appearance_auto_is_accepted_and_persisted() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--appearance", "auto", "--render-only", "--quiet"]);
    let status = sandbox.json(&["status", "--json"]);
    assert_eq!(status["state"]["appearanceMode"], "auto");
    assert_eq!(status["state"]["activeTheme"], "nerv-eva");
}

// ---------------------------------------------------------------------------
// Round 1 review regressions
// ---------------------------------------------------------------------------

#[test]
fn dry_run_uninstall_purge_leaves_every_file_intact() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    let zshrc_before = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    let ghostty_before = fs::read_to_string(sandbox.path("ghostty-config")).unwrap();
    let wt_before = fs::read_to_string(sandbox.path("wt-settings.json")).unwrap();
    let state_before = fs::read_to_string(sandbox.kiseki_home().join("state.json")).unwrap();

    let json = sandbox.json(&["uninstall", "--purge", "--dry-run", "--json"]);
    assert_eq!(json["purged"], true);
    assert_eq!(
        fs::read_to_string(sandbox.path(".zshrc")).unwrap(),
        zshrc_before
    );
    assert_eq!(
        fs::read_to_string(sandbox.path("ghostty-config")).unwrap(),
        ghostty_before
    );
    assert_eq!(
        fs::read_to_string(sandbox.path("wt-settings.json")).unwrap(),
        wt_before
    );
    assert_eq!(
        fs::read_to_string(sandbox.kiseki_home().join("state.json")).unwrap(),
        state_before
    );
    assert!(sandbox.kiseki_home().join("generated").exists());
    assert!(sandbox
        .kiseki_home()
        .join("data")
        .join("themes")
        .join("official")
        .join("nerv-eva")
        .exists());
}

#[test]
fn dry_run_theme_install_remove_and_font_lock_write_nothing() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/nerv-eva");
    let config_before = fs::read_to_string(sandbox.kiseki_home().join("config.toml")).unwrap_or_default();

    sandbox.json(&[
        "theme",
        "install",
        source.to_str().unwrap(),
        "--force",
        "--dry-run",
        "--json",
    ]);
    assert!(!sandbox
        .kiseki_home()
        .join("data")
        .join("themes")
        .join("community")
        .join("nerv-eva")
        .exists());

    sandbox.json(&["font", "JetBrainsMono Nerd Font", "--lock", "--dry-run", "--json"]);
    assert_eq!(
        fs::read_to_string(sandbox.kiseki_home().join("config.toml")).unwrap_or_default(),
        config_before
    );

    // Migration is planned but never executed.
    let legacy = sandbox.home().join(".config").join("kiseki-terminal");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("current-theme.txt"), "nerv-eva\n").unwrap();
    fs::write(
        sandbox.path(".zshrc"),
        "# user\n# >>> kiseki-terminal >>>\nsource old\n# <<< kiseki-terminal <<<\n",
    )
    .unwrap();
    let planned = sandbox.json(&["migrate", "--apply", "--dry-run", "--json"]);
    assert_eq!(planned["applied"], false);
    assert_eq!(planned["dryRun"], true);
    assert!(fs::read_to_string(sandbox.path(".zshrc"))
        .unwrap()
        .contains("# >>> kiseki-terminal >>>"));
}

#[test]
fn font_unlock_clears_both_the_config_and_state_flags() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    sandbox.run_ok(&[
        "font",
        "JetBrainsMono Nerd Font",
        "14",
        "--lock",
        "--render-only",
        "--quiet",
    ]);
    let state = sandbox.json(&["status", "--json"]);
    assert_eq!(state["state"]["typography"]["locked"], true);
    assert_eq!(state["config"]["fontLocked"], true);

    sandbox.run_ok(&["font", "JetBrainsMono Nerd Font", "--unlock", "--render-only", "--quiet"]);
    let state = sandbox.json(&["status", "--json"]);
    assert_eq!(state["state"]["typography"]["locked"], false);
    assert_eq!(state["config"]["fontLocked"], false);
    // The override itself survives the unlock until the next theme switch.
    assert_eq!(state["state"]["typography"]["sizeOverridePt"], 14.0);

    // Switching themes once unlocked lets the new theme supply its own font stack.
    sandbox.run_ok(&["use", "kyoto-sakura", "--render-only", "--quiet"]);
    let after_switch = sandbox.json(&["status", "--json"]);
    assert!(after_switch["state"]["typography"]["styleOverride"].is_null());
    assert!(after_switch["state"]["typography"]["sizeOverridePt"].is_null());
}

#[test]
fn enabled_adapters_intersect_with_only() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.kiseki_home()).unwrap();
    fs::write(
        sandbox.kiseki_home().join("config.toml"),
        "enabledAdapters = [\"ghostty\"]\n",
    )
    .unwrap();
    let zshrc_before = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    sandbox.run_ok(&["use", "nerv-eva", "--quiet"]);
    assert_eq!(fs::read_to_string(sandbox.path(".zshrc")).unwrap(), zshrc_before);
    assert!(fs::read_to_string(sandbox.path("ghostty-config"))
        .unwrap()
        .contains("# >>> kiseki >>>"));

    // An intersection that is empty runs zero adapters, not every adapter.
    sandbox.run_ok(&["use", "nerv-eva", "--only", "zsh", "--quiet"]);
    assert_eq!(fs::read_to_string(sandbox.path(".zshrc")).unwrap(), zshrc_before);
    let ghostty = fs::read_to_string(sandbox.path("ghostty-config")).unwrap();
    assert!(!ghostty.contains("_kiseki"), "{ghostty}");
}

#[test]
fn pick_writes_nothing_before_a_selection_is_confirmed() {
    let sandbox = Sandbox::new();
    let json = sandbox.json(&["pick", "--json"]);
    assert_eq!(json["cancelled"], true);
    let official = sandbox
        .kiseki_home()
        .join("data")
        .join("themes")
        .join("official");
    assert!(!official.join("nerv-eva").exists());
    assert!(!official.join(".versions.json").exists());
}

#[test]
fn theme_validate_json_emits_exactly_one_document_and_fails() {
    let sandbox = Sandbox::new();
    let dir = sandbox.dir.path().join("bad-pack");
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::write(dir.join("manifest.json"), "{\"id\": \"Bad\"}").unwrap();
    fs::write(dir.join("evil.lua"), "os.execute('rm -rf /')").unwrap();
    let output = sandbox.run(&[
        "theme",
        "validate",
        dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("expected exactly one JSON document ({error}):\n{stdout}")
    });
    assert_eq!(parsed["valid"], false);
    assert!(parsed.get("error").is_none(), "{parsed}");
    assert!(parsed["diagnostics"].as_array().unwrap().iter().any(|diagnostic| diagnostic["code"] == "security.file-not-allowed"));
}

#[test]
fn doctor_json_exits_nonzero_with_one_document_when_blocking() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.kiseki_home()).unwrap();
    fs::write(sandbox.kiseki_home().join("config.toml"), "not = [valid = toml").unwrap();
    let output = sandbox.run(&["doctor", "--json"]);
    assert!(!output.status.success(), "doctor must exit non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("expected exactly one JSON document ({error}):\n{stdout}")
    });
    assert_eq!(parsed["hasErrors"], true);
    assert!(parsed["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "config.parse" && check["status"] == "error"));
}

#[test]
fn recover_safe_starts_with_a_corrupt_state_file() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--profile", "ultra", "--render-only", "--quiet"]);
    fs::write(sandbox.kiseki_home().join("state.json"), "{ not json").unwrap();
    let json = sandbox.json(&["recover", "--safe", "--json"]);
    assert_eq!(json["state"]["rendering"]["profile"], "static");
    assert_eq!(json["state"]["activeTheme"], "nerv-eva");
    let state: Value = serde_json::from_str(
        &fs::read_to_string(sandbox.kiseki_home().join("state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["rendering"]["profile"], "static");
}

#[test]
fn recover_safe_falls_back_to_the_bundled_theme_when_the_pack_is_damaged() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    // Break the installed pack's tokens so loading it fails validation.
    let tokens = sandbox
        .kiseki_home()
        .join("data/themes/official/nerv-eva/tokens.json");
    fs::write(&tokens, "{ definitely not tokens }").unwrap();
    let json = sandbox.json(&["recover", "--safe", "--json"]);
    assert_eq!(json["state"]["rendering"]["profile"], "static");
    assert_eq!(json["state"]["activeTheme"], "nerv-eva");
    // The static artefacts were mounted before the state commit.
    assert!(sandbox
        .kiseki_home()
        .join("generated")
        .join("ghostty.conf")
        .exists());
    let ghostty = fs::read_to_string(sandbox.kiseki_home().join("generated/ghostty.conf")).unwrap();
    assert!(!ghostty.contains("custom-shader"), "{ghostty}");
}

#[test]
fn init_preserves_a_v1_static_profile() {
    let sandbox = Sandbox::new();
    let legacy = sandbox.home().join(".config").join("kiseki-terminal");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("current-theme.txt"), "nerv-eva\n").unwrap();
    fs::write(legacy.join("live-mode.txt"), "off\n").unwrap();
    let output = sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    assert!(output.status.success());
    let state = sandbox.json(&["status", "--json"]);
    assert_eq!(state["state"]["rendering"]["profile"], "static");
}

#[test]
fn compose_donor_assets_come_from_the_donor_pack() {
    let sandbox = Sandbox::new();
    // Install a second pack with a distinct wallpaper so the donor root is
    // observable in the generated Windows Terminal payload.
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/nerv-eva");
    sandbox.json(&[
        "theme",
        "install",
        source.to_str().unwrap(),
        "--force",
        "--json",
    ]);
    let community = sandbox.kiseki_home().join("data/themes/community/nerv-eva");
    let manifest = community.join("manifest.json");
    let mut value: Value =
        serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    value["id"] = serde_json::json!("donor-atmos");
    value["name"] = serde_json::json!("Donor Atmosphere");
    fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    // Move the directory to the donor id.
    let donor = sandbox.kiseki_home().join("data/themes/community/donor-atmos");
    fs::rename(&community, &donor).unwrap();

    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    sandbox.run_ok(&[
        "compose",
        "--atmosphere",
        "donor-atmos",
        "--render-only",
        "--quiet",
    ]);
    let payload: Value = serde_json::from_str(
        &fs::read_to_string(sandbox.kiseki_home().join("generated/wt-visual.json")).unwrap(),
    )
    .unwrap();
    let wallpaper = payload["defaults"]["experimental.pixelShaderImagePath"]
        .as_str()
        .unwrap();
    assert!(wallpaper.contains("donor-atmos"), "{wallpaper}");
    assert!(payload["defaults"].get("backgroundImage").is_none());
    let shader = payload["defaults"]["experimental.pixelShaderPath"]
        .as_str()
        .unwrap();
    assert!(shader.ends_with("active.hlsl"), "{shader}");
}

// ---------------------------------------------------------------------------
// Round 2 review regressions
// ---------------------------------------------------------------------------

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                visit(root, &path, out);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((relative, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    if root.exists() {
        visit(root, root, &mut out);
    }
    out
}

#[test]
fn recover_safe_dry_run_with_a_corrupt_state_and_a_journal_writes_nothing() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    assert!(sandbox
        .kiseki_home()
        .join("data")
        .join("journal")
        .read_dir()
        .unwrap()
        .any(|entry| entry.unwrap().path().is_dir()));
    fs::write(sandbox.kiseki_home().join("state.json"), "{ not json").unwrap();
    let before = snapshot(&sandbox.kiseki_home());

    let json = sandbox.json(&["recover", "--safe", "--dry-run", "--json"]);
    assert_eq!(json["dryRun"], true);
    assert_eq!(
        snapshot(&sandbox.kiseki_home()),
        before,
        "recover --safe --dry-run must not mutate a single file"
    );
}

#[test]
fn recover_safe_fails_when_the_generated_config_is_read_only() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&[
        "use",
        "nerv-eva",
        "--profile",
        "ultra",
        "--render-only",
        "--quiet",
    ]);
    let generated = sandbox.kiseki_home().join("generated/ghostty.conf");
    let before = fs::read_to_string(&generated).unwrap();
    assert!(before.contains("custom-shader"), "{before}");
    let mut permissions = fs::metadata(&generated).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&generated, permissions).unwrap();

    let output = sandbox.run(&["recover", "--safe", "--json"]);
    assert!(!output.status.success(), "a live shader must not be reported as recovered");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("expected one JSON document ({error}):\n{stdout}")
    });
    let targets = parsed["transaction"]["targets"].as_array().unwrap();
    assert!(
        targets.iter().any(|target| {
            target["target"].as_str().unwrap().ends_with("ghostty.conf")
                && matches!(
                    target["status"].as_str().unwrap(),
                    "failed" | "rolled-back"
                )
        }),
        "{targets:?}"
    );
    // The dynamic shader is indeed still live on disk.
    assert!(fs::read_to_string(&generated)
        .unwrap()
        .contains("custom-shader"));
}

#[test]
fn recover_safe_drops_a_missing_composition_donor() {
    let sandbox = Sandbox::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/nerv-eva");
    sandbox.json(&[
        "theme",
        "install",
        source.to_str().unwrap(),
        "--force",
        "--json",
    ]);
    let community = sandbox.kiseki_home().join("data/themes/community/nerv-eva");
    let manifest = community.join("manifest.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    value["id"] = serde_json::json!("donor-atmos");
    value["name"] = serde_json::json!("Donor Atmosphere");
    fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let donor = sandbox.kiseki_home().join("data/themes/community/donor-atmos");
    fs::rename(&community, &donor).unwrap();

    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    sandbox.run_ok(&[
        "compose",
        "--atmosphere",
        "donor-atmos",
        "--render-only",
        "--quiet",
    ]);
    fs::remove_dir_all(&donor).unwrap();

    let json = sandbox.json(&["recover", "--safe", "--json"]);
    assert_eq!(json["state"]["rendering"]["profile"], "static");
    assert!(json["state"]["composition"]["atmosphere"].is_null());
    let ghostty = fs::read_to_string(sandbox.kiseki_home().join("generated/ghostty.conf")).unwrap();
    assert!(!ghostty.contains("custom-shader"), "{ghostty}");
}

#[test]
fn uninstall_with_a_read_only_host_preserves_recovery_artifacts() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    assert!(fs::read_to_string(sandbox.path(".zshrc"))
        .unwrap()
        .contains("# >>> kiseki >>>"));

    for path in [sandbox.path(".zshrc"), sandbox.path("wt-settings.json")] {
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
    }

    let output = sandbox.run(&["uninstall", "--purge", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["transaction"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|target| target["status"] == "render-only"));

    // The host still references Kiseki, so the artifacts needed to retry stay.
    assert!(sandbox.kiseki_home().join("generated").exists());
    assert!(sandbox.kiseki_home().join("data").exists());
    assert!(sandbox
        .kiseki_home()
        .join("data/journal/wt-baseline.json")
        .exists());
    assert!(fs::read_to_string(sandbox.path(".zshrc"))
        .unwrap()
        .contains("# >>> kiseki >>>"));
}

#[test]
fn init_render_only_never_edits_v1_host_hooks() {
    let sandbox = Sandbox::new();
    let legacy = sandbox.home().join(".config").join("kiseki-terminal");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("current-theme.txt"), "nerv-eva\n").unwrap();
    fs::write(
        sandbox.path(".zshrc"),
        "# user\n# >>> kiseki-terminal >>>\nsource ~/.kiseki-terminal/env.zsh\n# <<< kiseki-terminal <<<\n",
    )
    .unwrap();

    sandbox.run_ok(&["init", "--non-interactive", "--render-only", "--quiet"]);

    let zshrc = fs::read_to_string(sandbox.path(".zshrc")).unwrap();
    assert!(zshrc.contains("# >>> kiseki-terminal >>>"), "{zshrc}");
    assert!(!zshrc.contains("# >>> kiseki >>>"), "{zshrc}");
    // The legacy directory was still backed up and artifacts were generated.
    let journal = sandbox.kiseki_home().join("data/journal");
    let migration = fs::read_dir(&journal)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("v1-migration-"))
        })
        .expect("a migration backup dir exists");
    assert!(migration.join("legacy/current-theme.txt").exists());
    assert!(sandbox.kiseki_home().join("generated/env.zsh").exists());
}

#[test]
fn init_falls_back_when_the_v1_theme_is_not_available() {
    let sandbox = Sandbox::new();
    let legacy = sandbox.home().join(".config").join("kiseki-terminal");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("current-theme.txt"), "retired-v1-theme\n").unwrap();
    sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    let status = sandbox.json(&["status", "--json"]);
    assert_eq!(status["state"]["activeTheme"], "nerv-eva");
    assert!(sandbox.kiseki_home().join("generated/ghostty.conf").exists());
}

#[test]
fn init_preserves_official_v1_theme() {
    let sandbox = Sandbox::new();
    let legacy = sandbox.home().join(".config").join("kiseki-terminal");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("current-theme.txt"), "kyoto-sakura\n").unwrap();
    sandbox.run_ok(&["init", "--non-interactive", "--quiet"]);
    let status = sandbox.json(&["status", "--json"]);
    assert_eq!(status["state"]["activeTheme"], "kyoto-sakura");
    assert!(sandbox.kiseki_home().join("generated/ghostty.conf").exists());
}

#[test]
fn json_startup_errors_emit_an_error_document() {
    let sandbox = Sandbox::new();
    // Corrupt `state.json` fails `App::new` startup consistently on every OS.
    fs::create_dir_all(sandbox.kiseki_home()).unwrap();
    fs::write(sandbox.kiseki_home().join("state.json"), "{ not json").unwrap();
    let output = sandbox
        .command()
        .args(["--json", "doctor"])
        .output()
        .expect("kiseki binary runs");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("expected an error JSON document ({error}):\n{stdout}")
    });
    assert_eq!(parsed["ok"], false);
    assert!(
        parsed["error"].as_str().unwrap().contains("state.json"),
        "{parsed}"
    );
}

#[test]
fn powershell_completions_are_pure_ascii_for_ps51() {
    let sandbox = Sandbox::new();
    let output = sandbox.run_ok(&["completions", "powershell"]);
    let script = String::from_utf8(output.stdout).expect("valid UTF-8");
    assert!(script.is_ascii(), "PowerShell completions must be pure ASCII for PS 5.1");
    assert!(script.contains("Register-ArgumentCompleter"));
}

#[cfg(windows)]
#[test]
fn windows_init_and_uninstall_manage_both_ps51_and_ps7_profiles() {
    let sandbox = Sandbox::new();
    let ps7 = sandbox
        .home()
        .join("Documents")
        .join("PowerShell")
        .join("Microsoft.PowerShell_profile.ps1");
    let ps51 = sandbox
        .home()
        .join("Documents")
        .join("WindowsPowerShell")
        .join("Microsoft.PowerShell_profile.ps1");
    fs::create_dir_all(ps7.parent().unwrap()).unwrap();
    fs::create_dir_all(ps51.parent().unwrap()).unwrap();
    fs::write(&ps7, "# existing ps7\n").unwrap();
    fs::write(&ps51, "# existing ps51\n").unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiseki"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("USERPROFILE", sandbox.home())
        .env("LOCALAPPDATA", sandbox.home().join("AppData").join("Local"))
        .env("KISEKI_HOME", sandbox.kiseki_home())
        .env("KISEKI_WT_SETTINGS", sandbox.path("wt-settings.json"))
        .env("NO_COLOR", "1")
        .env("KISEKI_APPEARANCE", "dark")
        .env("KISEKI_POWER_SOURCE", "ac")
        .args(["init", "--non-interactive", "--aliases", "--quiet"]);
    let out = cmd.output().expect("kiseki init runs");
    assert!(out.status.success(), "{out:?}");

    let ps7_after = fs::read_to_string(&ps7).unwrap();
    let ps51_after = fs::read_to_string(&ps51).unwrap();
    assert!(ps7_after.contains("# existing ps7"), "{ps7_after}");
    assert!(ps7_after.contains("# >>> kiseki >>>"), "{ps7_after}");
    assert!(ps51_after.contains("# existing ps51"), "{ps51_after}");
    assert!(ps51_after.contains("# >>> kiseki >>>"), "{ps51_after}");

    let env_ps1 = fs::read(sandbox.kiseki_home().join("generated/env.ps1")).unwrap();
    assert_eq!(&env_ps1[..3], &[0xEF, 0xBB, 0xBF], "env.ps1 must have UTF-8 BOM");

    let mut uninstall = Command::new(env!("CARGO_BIN_EXE_kiseki"));
    uninstall
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("USERPROFILE", sandbox.home())
        .env("LOCALAPPDATA", sandbox.home().join("AppData").join("Local"))
        .env("KISEKI_HOME", sandbox.kiseki_home())
        .env("KISEKI_WT_SETTINGS", sandbox.path("wt-settings.json"))
        .env("NO_COLOR", "1")
        .args(["uninstall", "--purge", "--quiet"]);
    let out = uninstall.output().expect("kiseki uninstall runs");
    assert!(out.status.success(), "{out:?}");

    let ps7_restored = fs::read_to_string(&ps7).unwrap();
    let ps51_restored = fs::read_to_string(&ps51).unwrap();
    assert!(!ps7_restored.contains("# >>> kiseki >>>"), "{ps7_restored}");
    assert!(ps7_restored.contains("# existing ps7"), "{ps7_restored}");
    assert!(!ps51_restored.contains("# >>> kiseki >>>"), "{ps51_restored}");
    assert!(ps51_restored.contains("# existing ps51"), "{ps51_restored}");
}

#[test]
fn doctor_reports_a_broken_active_pack() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--render-only", "--quiet"]);
    let tokens = sandbox
        .kiseki_home()
        .join("data/themes/official/nerv-eva/tokens.json");
    fs::write(&tokens, "{ definitely not tokens }").unwrap();

    let output = sandbox.run(&["doctor", "--json"]);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("expected one JSON document ({error}):\n{stdout}")
    });
    assert_eq!(parsed["hasErrors"], true);
    let check = parsed["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "theme.validate")
        .expect("theme.validate is reported");
    assert_eq!(check["status"], "error");
    assert!(check["message"].as_str().unwrap().contains("cannot be loaded"));
}

#[test]
fn theme_install_dry_run_rejects_an_invalid_pack_with_one_json_document() {
    let sandbox = Sandbox::new();
    let dir = sandbox.dir.path().join("invalid-pack");
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::write(dir.join("manifest.json"), "{\"id\": \"Bad\"}").unwrap();
    fs::write(dir.join("tokens.json"), "{}").unwrap();
    fs::write(dir.join("LICENSE"), "MIT").unwrap();
    fs::write(dir.join("CREDITS.md"), "credits").unwrap();
    fs::write(dir.join("evil.lua"), "os.execute('rm -rf /')").unwrap();

    let output = sandbox.run(&[
        "theme",
        "install",
        dir.to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);
    assert!(!output.status.success(), "an invalid pack must exit non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("expected one JSON document ({error}):\n{stdout}")
    });
    assert_eq!(parsed["valid"], false);
    assert!(parsed.get("error").is_none(), "{parsed}");
    // Nothing was installed.
    assert!(!sandbox
        .kiseki_home()
        .join("data/themes/community/Bad")
        .exists());
}

#[test]
fn rollback_after_recover_safe_restores_previous_profile() {
    let sandbox = Sandbox::new();
    sandbox.run_ok(&["use", "nerv-eva", "--profile", "ultra", "--render-only", "--quiet"]);
    let before = sandbox.json(&["status", "--json"]);
    assert_eq!(before["state"]["rendering"]["profile"], "ultra");

    sandbox.run_ok(&["recover", "--safe", "--render-only", "--quiet"]);
    let recovered = sandbox.json(&["status", "--json"]);
    assert_eq!(recovered["state"]["rendering"]["profile"], "static");

    sandbox.run_ok(&["rollback", "--quiet"]);
    let restored = sandbox.json(&["status", "--json"]);
    assert_eq!(restored["state"]["rendering"]["profile"], "ultra");
}

#[cfg(unix)]
#[test]
fn uninstall_purge_reports_false_when_host_is_read_only() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let ghostty = sandbox.home().join(".config/ghostty/config.ghostty");
    fs::create_dir_all(ghostty.parent().unwrap()).unwrap();
    fs::write(&ghostty, "# user ghostty\n").unwrap();

    let status = sandbox
        .command()
        .env("KISEKI_GHOSTTY_CONFIG", &ghostty)
        .args(["use", "nerv-eva", "--only", "ghostty", "--quiet"])
        .status()
        .expect("kiseki runs");
    assert!(status.success());

    fs::set_permissions(&ghostty, fs::Permissions::from_mode(0o444)).unwrap();
    let output = sandbox
        .command()
        .env("KISEKI_GHOSTTY_CONFIG", &ghostty)
        .args(["uninstall", "--purge", "--json"])
        .output()
        .expect("kiseki runs");
    fs::set_permissions(&ghostty, fs::Permissions::from_mode(0o644)).unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("expected valid json ({err}):\n{stdout}")
    });
    assert_eq!(parsed["purged"], false);
    assert_eq!(parsed["incomplete"], true);
    assert!(sandbox.kiseki_home().join("data").exists());
}
