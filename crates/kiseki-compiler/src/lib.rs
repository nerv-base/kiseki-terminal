//! Deterministic compilation from a [`ResolvedTheme`] to an [`ArtifactMap`].
//!
//! This crate is a pure function library. It never reads or writes the host
//! filesystem and never spawns a process; the caller supplies every external
//! input (shader templates, logo text, resolved font families, the config
//! directory used for absolute references). That makes the whole pipeline
//! unit-testable and byte-for-byte reproducible.

pub mod artifact;
pub mod escape;
pub mod fastfetch;
pub mod ghostty;
pub mod marker;
pub mod prompt;
pub mod shell;
pub mod windows_terminal;
pub mod zsh_assets;

use std::path::PathBuf;

use kiseki_schema::ResolvedTheme;
use kiseki_types::{Diagnostic, DiagnosticReport, KisekiError, PromptPolicy, Result};

pub use artifact::{ArtifactMap, ArtifactPath};
pub use artifact::names as artifact_names;
pub use shell::ShellKind;
pub use windows_terminal::WtVisualPayload;

/// Everything outside the theme that affects the compiled bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOptions {
    /// `<kiseki_config>`; generated files reference it with absolute paths.
    pub config_dir: PathBuf,
    /// Directory containing `manifest.json`, `tokens.json`, `assets/` and
    /// `shaders/` for this theme.
    pub theme_root: PathBuf,
    /// Directory that supplies the L3 atmosphere assets (wallpaper, shaders)
    /// when `kiseki compose --atmosphere` donates them. Defaults to
    /// [`Self::theme_root`].
    pub atmosphere_dir: Option<PathBuf>,
    /// Directory that supplies the L5 overrides assets (logo). Defaults to
    /// [`Self::theme_root`].
    pub overrides_dir: Option<PathBuf>,
    /// Directory for the standalone Windows installer's private executables.
    pub windows_vendor_bin: Option<PathBuf>,
    /// How prompt artefacts should be produced.
    pub prompt_policy: PromptPolicy,
    /// Whether opt-in `theme` / `themes` aliases are added to shell artefacts.
    pub aliases: bool,
    /// Concrete font families to write into terminal configs. Empty means
    /// "use the theme's declared families verbatim".
    pub font_families: Vec<String>,
    /// Scale theme default DirectWrite point size (~96 DPI) to macOS CoreText
    /// (~72 DPI, 1.25x rounded to 0.5pt) when the user did not explicitly
    /// override font size.
    pub macos_font_scale: bool,
    /// Skip redundant `naga` validation inside `compile` when the caller
    /// already validated the exact `(theme, profile)` shader templates.
    pub skip_shader_validation: bool,
}

impl CompileOptions {
    pub fn new(config_dir: impl Into<PathBuf>, theme_root: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
            theme_root: theme_root.into(),
            atmosphere_dir: None,
            overrides_dir: None,
            windows_vendor_bin: None,
            prompt_policy: PromptPolicy::ManagedTemplate,
            aliases: false,
            font_families: Vec::new(),
            macos_font_scale: false,
            skip_shader_validation: false,
        }
    }

    pub fn with_skip_shader_validation(mut self, skip: bool) -> Self {
        self.skip_shader_validation = skip;
        self
    }

    pub fn with_macos_font_scale(mut self, enabled: bool) -> Self {
        self.macos_font_scale = enabled;
        self
    }

    pub fn with_prompt_policy(mut self, policy: PromptPolicy) -> Self {
        self.prompt_policy = policy;
        self
    }

    pub fn with_aliases(mut self, aliases: bool) -> Self {
        self.aliases = aliases;
        self
    }

    pub fn with_font_families(mut self, families: Vec<String>) -> Self {
        self.font_families = families;
        self
    }

    /// Sets the donor root for L3 atmosphere assets (wallpaper + shaders).
    pub fn with_atmosphere_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.atmosphere_dir = dir;
        self
    }

    /// Sets the donor root for L5 override assets (the fastfetch logo).
    pub fn with_overrides_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.overrides_dir = dir;
        self
    }

    /// Root that owns the wallpaper and shader templates.
    pub fn atmosphere_root(&self) -> &std::path::Path {
        self.atmosphere_dir.as_deref().unwrap_or(&self.theme_root)
    }

    /// Root that owns the logo asset.
    pub fn overrides_root(&self) -> &std::path::Path {
        self.overrides_dir.as_deref().unwrap_or(&self.theme_root)
    }

    fn effective_families(&self, theme: &ResolvedTheme) -> Vec<String> {
        if self.font_families.is_empty() {
            theme.typography.families.clone()
        } else {
            self.font_families.clone()
        }
    }
}

/// All external inputs to one compilation.
#[derive(Debug, Clone)]
pub struct CompileInput<'a> {
    pub theme: &'a ResolvedTheme,
    pub options: CompileOptions,
    /// Contents of `shaders/live.glsl`, when the theme ships one.
    pub glsl_template: Option<&'a str>,
    /// Contents of `shaders/live.hlsl`, when the theme ships one.
    pub hlsl_template: Option<&'a str>,
    /// Contents of `assets/logo.txt`, when the theme ships one.
    pub logo_text: Option<&'a str>,
}

impl<'a> CompileInput<'a> {
    pub fn new(theme: &'a ResolvedTheme, options: CompileOptions) -> Self {
        Self {
            theme,
            options,
            glsl_template: None,
            hlsl_template: None,
            logo_text: None,
        }
    }

    pub fn with_glsl(mut self, template: Option<&'a str>) -> Self {
        self.glsl_template = template;
        self
    }

    pub fn with_hlsl(mut self, template: Option<&'a str>) -> Self {
        self.hlsl_template = template;
        self
    }

    pub fn with_logo(mut self, logo: Option<&'a str>) -> Self {
        self.logo_text = logo;
        self
    }
}

/// Compiled artefacts plus non-fatal findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutput {
    pub artifacts: ArtifactMap,
    pub diagnostics: DiagnosticReport,
}

/// Compiles a resolved theme into every generated artefact.
pub fn compile(input: &CompileInput<'_>) -> Result<CompileOutput> {
    let theme = input.theme;
    let options = &input.options;
    let mut artifacts = ArtifactMap::new();
    let mut diagnostics = DiagnosticReport::new();
    let families = options.effective_families(theme);

    let wallpaper = theme
        .wallpaper_path()
        .map(|path| join_relative(options.atmosphere_root(), path));
    let logo_path = logo_path(theme, options, input.logo_text);
    if logo_path.is_none() {
        if let Some(fastfetch) = theme
            .overrides
            .as_ref()
            .and_then(|overrides| overrides.fastfetch.as_ref())
        {
            if fastfetch.logo_path.is_some() {
                diagnostics.push(
                    Diagnostic::warning(
                        "compiler.fastfetch.logo-missing",
                        "theme declares a fastfetch logo but the asset was not loaded; the logo block is omitted",
                    )
                    .with_hint("run `kiseki theme validate <path>` to verify pack assets"),
                );
            }
        }
    }

    // --- shaders ---------------------------------------------------------
    let mut glsl_artifact: Option<PathBuf> = None;
    let mut hlsl_artifact: Option<PathBuf> = None;
    if theme.shader_enabled {
        if theme.glsl_path().is_some() {
            let template = input.glsl_template.ok_or_else(|| {
                KisekiError::compiler(format!(
                    "theme '{}' declares {} but no GLSL template was loaded",
                    theme.id,
                    theme.glsl_path().unwrap_or_default()
                ))
            })?;
            if !options.skip_shader_validation {
                let report = kiseki_shader::validate_glsl(theme, template);
                if report.has_fatal() {
                    return Err(KisekiError::shader(format!(
                        "GLSL template rejected at {}: {}",
                        theme.profile,
                        summarize(&report.diagnostics)
                    )));
                }
            }
            let program = kiseki_shader::compile_glsl_for(theme, template);
            artifacts.insert(artifact::names::SHADER_GLSL, program.source)?;
            glsl_artifact = Some(join_relative(
                &options.config_dir.join("generated"),
                artifact::names::SHADER_GLSL,
            ));
        }
        if theme.hlsl_path().is_some() {
            let template = input.hlsl_template.ok_or_else(|| {
                KisekiError::compiler(format!(
                    "theme '{}' declares {} but no HLSL template was loaded",
                    theme.id,
                    theme.hlsl_path().unwrap_or_default()
                ))
            })?;
            if !options.skip_shader_validation {
                let report = kiseki_shader::validate_hlsl(theme, template);
                if report.has_fatal() {
                    return Err(KisekiError::shader(format!(
                        "HLSL template rejected at {}: {}",
                        theme.profile,
                        summarize(&report.diagnostics)
                    )));
                }
            }
            let program = kiseki_shader::compile_hlsl_for(theme, template);
            artifacts.insert(artifact::names::SHADER_HLSL, program.source)?;
            hlsl_artifact = Some(join_relative(
                &options.config_dir.join("generated"),
                artifact::names::SHADER_HLSL,
            ));
        }
    } else {
        diagnostics.push(Diagnostic::info(
            "compiler.shader.disabled",
            format!("profile {} mounts no dynamic shader", theme.profile),
        ));
    }

    // --- terminal configs ------------------------------------------------
    let ghostty = ghostty::render_with_scale(
        theme,
        &families,
        wallpaper.as_deref(),
        glsl_artifact.as_deref(),
        options.macos_font_scale,
    )?;
    diagnostics.extend(ghostty::lint(&ghostty));
    artifacts.insert(artifact::names::GHOSTTY_CONF, ghostty)?;

    let payload = windows_terminal::build(
        theme,
        &families,
        wallpaper.as_deref(),
        hlsl_artifact.as_deref(),
    );
    diagnostics.extend(payload.lint());
    artifacts.insert(
        artifact::names::WT_VISUAL,
        serde_json::to_string_pretty(&payload).map_err(|error| KisekiError::Json(error.to_string()))?,
    )?;

    // --- shell environments ---------------------------------------------
    let zsh = shell::env_zsh(theme, &options.config_dir, options.prompt_policy, options.aliases);
    diagnostics.extend(shell::lint_shell_artifacts(&zsh));
    artifacts.insert(artifact::names::ENV_ZSH, zsh)?;

    let ps1 = match options.windows_vendor_bin.as_deref() {
        Some(vendor_bin) => shell::env_ps1_with_vendor(
            theme,
            &options.config_dir,
            options.prompt_policy,
            vendor_bin,
        ),
        None => shell::env_ps1(theme, &options.config_dir, options.prompt_policy),
    };
    diagnostics.extend(shell::lint_shell_artifacts(&ps1));
    artifacts.insert(artifact::names::ENV_PS1, ps1)?;

    // --- prompt ----------------------------------------------------------
    match options.prompt_policy {
        PromptPolicy::ManagedTemplate => {}
        PromptPolicy::PaletteOnly => diagnostics.push(Diagnostic::info(
            "compiler.prompt.palette-only",
            "user prompt layout kept; only palette artefacts were written",
        )),
        PromptPolicy::Disabled => diagnostics.push(Diagnostic::info(
            "compiler.prompt.disabled",
            "prompt artefacts disabled by config",
        )),
    }
    for (path, content) in prompt::artifacts(theme, options.prompt_policy) {
        artifacts.insert(path, content)?;
    }

    // --- fastfetch -------------------------------------------------------
    let fastfetch = fastfetch::render(theme, logo_path.as_deref());
    artifacts.insert(artifact::names::FASTFETCH, fastfetch)?;

    Ok(CompileOutput {
        artifacts,
        diagnostics,
    })
}

fn join_relative(base: &std::path::Path, relative: &str) -> PathBuf {
    let mut path = base.to_path_buf();
    for segment in relative.split('/').filter(|segment| !segment.is_empty()) {
        path.push(segment);
    }
    path
}

fn logo_path(
    theme: &ResolvedTheme,
    options: &CompileOptions,
    logo_text: Option<&str>,
) -> Option<PathBuf> {
    logo_text?;
    let declared = theme
        .overrides
        .as_ref()
        .and_then(|overrides| overrides.fastfetch.as_ref())
        .and_then(|fastfetch| fastfetch.logo_path.as_deref())
        .unwrap_or("assets/logo.txt");
    Some(join_relative(options.overrides_root(), declared))
}

fn summarize(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_fatal())
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Compiles the zero-fork hook for `kiseki shell-init`.
pub fn shell_init_script(shell: ShellKind, config_dir: &std::path::Path, aliases: bool) -> String {
    shell::init_script(shell, config_dir, aliases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiseki_schema::{ThemePack, ResolvedTheme};
    use kiseki_types::RenderingProfile;
    use std::path::Path;

    const GLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.glsl");
    const HLSL: &str = include_str!("../../../themes/nerv-eva/shaders/live.hlsl");
    const LOGO: &str = include_str!("../../../themes/nerv-eva/assets/logo.txt");

    fn pack() -> ThemePack {
        ThemePack {
            manifest: serde_json::from_str(include_str!("../../../themes/nerv-eva/manifest.json")).unwrap(),
            tokens: serde_json::from_str(include_str!("../../../themes/nerv-eva/tokens.json")).unwrap(),
        }
    }

    fn theme(profile: RenderingProfile) -> ResolvedTheme {
        ResolvedTheme::from_pack(&pack(), profile)
    }

    fn options() -> CompileOptions {
        #[cfg(windows)]
        let (config_dir, theme_root) = (r"C:\Users\zz\.config\kiseki", r"C:\data\themes\nerv-eva");
        #[cfg(not(windows))]
        let (config_dir, theme_root) = ("/home/zz/.config/kiseki", "/data/themes/nerv-eva");
        CompileOptions::new(config_dir, theme_root)
            .with_font_families(vec!["JetBrainsMono Nerd Font".to_string()])
    }

    fn input(profile: RenderingProfile) -> (ResolvedTheme, CompileOptions) {
        (theme(profile), options())
    }

    fn compile_at(profile: RenderingProfile) -> CompileOutput {
        let (theme, options) = input(profile);
        compile(
            &CompileInput::new(&theme, options)
                .with_glsl(Some(GLSL))
                .with_hlsl(Some(HLSL))
                .with_logo(Some(LOGO)),
        )
        .unwrap()
    }

    #[test]
    fn balanced_compiles_every_artifact() {
        let output = compile_at(RenderingProfile::Balanced);
        for path in [
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
            assert!(output.artifacts.contains(path), "missing {path}");
        }
        let glsl = output.artifacts.get("shaders/active.glsl").unwrap();
        assert!(glsl.contains("KISEKI_QUALITY_LEVEL 2"));
        // Ghostty supplies its own preamble and `main()`.
        assert!(glsl.contains("mainImage"));
        assert!(!glsl.contains("#version"));
        assert!(!glsl.contains("void main()"));
        assert!(output.artifacts.get("shaders/active.hlsl").unwrap().contains("46.0000"));
        assert!(!output.diagnostics.has_fatal());
    }

    #[test]
    fn every_dynamic_profile_compiles_with_valid_shaders() {
        for profile in [
            RenderingProfile::Eco,
            RenderingProfile::Balanced,
            RenderingProfile::Ultra,
        ] {
            let output = compile_at(profile);
            assert!(output.artifacts.contains("shaders/active.glsl"), "{profile}");
            assert!(output.artifacts.contains("shaders/active.hlsl"), "{profile}");
            assert!(!output.diagnostics.has_fatal(), "{profile}");
            let level = profile.quality_level().to_string();
            assert!(output
                .artifacts
                .get("shaders/active.glsl")
                .unwrap()
                .contains(&format!("#define KISEKI_QUALITY_LEVEL {level}")));
        }
    }

    #[test]
    fn static_and_accessibility_profiles_emit_no_shader() {
        for profile in [RenderingProfile::Static, RenderingProfile::Accessibility] {
            let output = compile_at(profile);
            assert!(!output.artifacts.contains("shaders/active.glsl"), "{profile}");
            assert!(!output.artifacts.contains("shaders/active.hlsl"), "{profile}");
            let ghostty = output.artifacts.get("ghostty.conf").unwrap();
            assert!(!ghostty.contains("custom-shader"));
            let payload = WtVisualPayload::from_json(output.artifacts.get("wt-visual.json").unwrap()).unwrap();
            assert!(payload
                .defaults
                .keys()
                .all(|key| !key.starts_with("experimental")));
        }
    }

    #[test]
    fn accessibility_removes_wallpaper_and_transparency() {
        let output = compile_at(RenderingProfile::Accessibility);
        let ghostty = output.artifacts.get("ghostty.conf").unwrap();
        assert!(!ghostty.contains("background-image"));
        assert!(!ghostty.contains("background-blur = true"));
        assert!(ghostty.contains("background-opacity = 1.0000"));
    }

    #[test]
    fn compilation_is_byte_for_byte_deterministic() {
        let first = compile_at(RenderingProfile::Ultra);
        let second = compile_at(RenderingProfile::Ultra);
        assert_eq!(first, second);
        assert_eq!(first.artifacts.paths(), second.artifacts.paths());

        let (theme, opts) = input(RenderingProfile::Ultra);
        let skipped = compile(
            &CompileInput::new(&theme, opts.with_skip_shader_validation(true))
                .with_glsl(Some(GLSL))
                .with_hlsl(Some(HLSL))
                .with_logo(Some(LOGO)),
        )
        .unwrap();
        assert_eq!(first.artifacts, skipped.artifacts);
    }

    #[test]
    fn fastfetch_logo_is_absolute() {
        let output = compile_at(RenderingProfile::Balanced);
        let raw = output.artifacts.get("fastfetch.jsonc").unwrap();
        let parsed = fastfetch::parse_body(raw).unwrap();
        let logo = parsed["logo"]["source"].as_str().unwrap();
        assert!(Path::new(logo).is_absolute(), "{logo}");
        assert!(logo.replace('\\', "/").ends_with("/assets/logo.txt"), "{logo}");
        #[cfg(windows)]
        assert!(!logo.contains('/'), "mixed separators on Windows: {logo}");
        assert!(fastfetch::is_declarative(&parsed));
    }

    #[test]
    fn prompt_has_no_command_segments() {
        let output = compile_at(RenderingProfile::Balanced);
        let raw = output.artifacts.get("prompt.omp.json").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert!(prompt::omp_is_declarative(&parsed));
        assert!(!raw.contains("\"command\""));
    }

    #[test]
    fn prompt_policy_controls_artifacts() {
        let (theme, options) = input(RenderingProfile::Balanced);
        let output = compile(
            &CompileInput::new(&theme, options.with_prompt_policy(PromptPolicy::Disabled))
                .with_glsl(Some(GLSL))
                .with_hlsl(Some(HLSL)),
        )
        .unwrap();
        assert!(!output.artifacts.contains("prompt.omp.json"));
        assert!(!output.artifacts.contains("starship-palette.toml"));
        let zsh = output.artifacts.get("env.zsh").unwrap();
        assert!(!zsh.contains("POSH_THEME"));
        assert!(output
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "compiler.prompt.disabled"));
    }

    #[test]
    fn missing_shader_template_is_a_hard_error() {
        let theme = theme(RenderingProfile::Balanced);
        let error = compile(&CompileInput::new(&theme, options())).unwrap_err();
        assert!(error.to_string().contains("GLSL template"), "{error}");
    }

    #[test]
    fn invalid_shader_template_is_rejected_before_emitting() {
        let theme = theme(RenderingProfile::Balanced);
        let bad = "void mainImage(out vec4 c, in vec2 p) { c = vec4(smoothstep(0.9, 0.1, p.x)); }";
        let error = compile(
            &CompileInput::new(&theme, options())
                .with_glsl(Some(bad))
                .with_hlsl(Some(HLSL)),
        )
        .unwrap_err();
        assert!(error.to_string().contains("smoothstep"), "{error}");
    }

    #[test]
    fn shell_init_is_available_for_both_shells() {
        let zsh = shell_init_script(ShellKind::Zsh, Path::new("/home/zz/.config/kiseki"), false);
        assert!(zsh.contains("_kiseki_sync_hook"));
        let pwsh = shell_init_script(ShellKind::Pwsh, Path::new("/home/zz/.config/kiseki"), false);
        assert!(pwsh.contains("_KisekiSyncHook"));
    }

    #[test]
    fn donor_asset_roots_supply_wallpaper_shaders_and_logo() {
        // `kiseki compose --atmosphere` / `--overrides`: the resolved theme
        // declares the *donor's* paths, so the compile options must point at the
        // donor's directory rather than the active theme's.
        let (theme, options) = input(RenderingProfile::Balanced);
        #[cfg(windows)]
        let donor = PathBuf::from(r"C:\data\themes\shinkai-rain");
        #[cfg(not(windows))]
        let donor = PathBuf::from("/data/themes/shinkai-rain");
        let output = compile(
            &CompileInput::new(
                &theme,
                options
                    .with_atmosphere_dir(Some(donor.clone()))
                    .with_overrides_dir(Some(donor.clone())),
            )
            .with_glsl(Some(GLSL))
            .with_hlsl(Some(HLSL))
            .with_logo(Some(LOGO)),
        )
        .unwrap();
        let payload = WtVisualPayload::from_json(output.artifacts.get("wt-visual.json").unwrap()).unwrap();
        let wallpaper = payload.defaults["experimental.pixelShaderImagePath"]
            .as_str()
            .unwrap();
        assert!(
            wallpaper.starts_with(&donor.to_string_lossy().to_string()),
            "{wallpaper}"
        );
        // Live shader mode composites the wallpaper in t1 and omits static `backgroundImage`.
        assert!(!payload.defaults.contains_key("backgroundImage"));
        let ghostty = output.artifacts.get("ghostty.conf").unwrap();
        assert!(ghostty.contains(&donor.to_string_lossy().to_string()), "{ghostty}");
        let fastfetch = fastfetch::parse_body(output.artifacts.get("fastfetch.jsonc").unwrap()).unwrap();
        let logo = fastfetch["logo"]["source"].as_str().unwrap();
        assert!(logo.starts_with(&donor.to_string_lossy().to_string()), "{logo}");

        // Static profile (no shader) carries the donor wallpaper on `backgroundImage`.
        let (static_theme, static_options) = input(RenderingProfile::Static);
        let static_output = compile(
            &CompileInput::new(
                &static_theme,
                static_options.with_atmosphere_dir(Some(donor.clone())),
            )
            .with_logo(Some(LOGO)),
        )
        .unwrap();
        let static_payload =
            WtVisualPayload::from_json(static_output.artifacts.get("wt-visual.json").unwrap()).unwrap();
        let static_wall = static_payload.defaults["backgroundImage"].as_str().unwrap();
        assert!(
            static_wall.starts_with(&donor.to_string_lossy().to_string()),
            "{static_wall}"
        );

        // Without a donor the active theme root is used.
        let (theme, options) = input(RenderingProfile::Balanced);
        assert_eq!(options.atmosphere_root(), options.theme_root);
        assert_eq!(options.overrides_root(), options.theme_root);
        let _ = theme;
    }

    #[test]
    fn artifacts_never_contain_shell_integration_or_keybinds() {
        let output = compile_at(RenderingProfile::Balanced);
        for path in output.artifacts.paths() {
            let content = output.artifacts.get(path).unwrap();
            assert!(!content.contains("shell-integration"), "{path}");
            assert!(!content.contains("keybind"), "{path}");
        }
    }
}
