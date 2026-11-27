//! External tools and release assets (RFC-0005).

use std::path::{Path, PathBuf};

use kiseki_types::OsKind;
use serde::Serialize;

use crate::Platform;

pub const OMP_VERSION: &str = "33.4.1";
// GitHub fastfetch-cli/fastfetch releases/latest on 2026-11-26.
pub const FASTFETCH_VERSION: &str = "2.71.0";
pub const JETBRAINS_MONO_NF_TAG: &str = "v3.5.1";

pub const INSTALLER_REMEDY: &str = "重跑安装脚本（见 https://kisekiterm.dev）";

pub const SYNTAX_HIGHLIGHTING_PATHS: &[&str] = &[
    "/opt/homebrew/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh",
    "/usr/local/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh",
    "/usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh",
    "/usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh",
    "/home/linuxbrew/.linuxbrew/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh",
];
pub const AUTOSUGGESTIONS_PATHS: &[&str] = &[
    "/opt/homebrew/share/zsh-autosuggestions/zsh-autosuggestions.zsh",
    "/usr/local/share/zsh-autosuggestions/zsh-autosuggestions.zsh",
    "/usr/share/zsh-autosuggestions/zsh-autosuggestions.zsh",
    "/usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh",
    "/home/linuxbrew/.linuxbrew/share/zsh-autosuggestions/zsh-autosuggestions.zsh",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    OnPath,
    KnownPaths(&'static [&'static str]),
    KnownPathsOrVendored(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRange {
    Unrestricted,
    /// The prompt uses a config API version independent of the binary version.
    TemplateApi(u64),
    /// Stable semantic versions in the specified major series.
    Major(u64),
}

impl VersionRange {
    pub fn accepts_binary(self, version: &str) -> bool {
        match self {
            Self::Major(major) => {
                let release = version.split('+').next().unwrap_or(version);
                !release.contains('-')
                    && version_parts(release).is_some_and(|parts| parts[0] == major)
            }
            Self::TemplateApi(_) | Self::Unrestricted => true,
        }
    }
}

#[derive(Debug)]
pub struct ToolSpec {
    pub id: &'static str,
    pub probe: ProbeKind,
    pub brew_package: Option<&'static str>,
    pub version_range: VersionRange,
    pub capability: &'static str,
    pub remedy: &'static str,
    pub installer_asset: bool,
}

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        id: "oh-my-posh",
        probe: ProbeKind::OnPath,
        brew_package: Some("oh-my-posh"),
        version_range: VersionRange::TemplateApi(3),
        capability: "主题 Prompt",
        remedy: INSTALLER_REMEDY,
        installer_asset: true,
    },
    ToolSpec {
        id: "fastfetch",
        probe: ProbeKind::OnPath,
        brew_package: Some("fastfetch"),
        version_range: VersionRange::Major(2),
        capability: "主题系统信息卡",
        remedy: INSTALLER_REMEDY,
        installer_asset: true,
    },
    ToolSpec {
        id: "eza",
        probe: ProbeKind::OnPath,
        brew_package: None,
        version_range: VersionRange::Unrestricted,
        capability: "文件列表配色",
        remedy: "安装 eza（Homebrew 使用 `brew install eza`）",
        installer_asset: false,
    },
    ToolSpec {
        id: "bat",
        probe: ProbeKind::OnPath,
        brew_package: None,
        version_range: VersionRange::Unrestricted,
        capability: "文件预览配色",
        remedy: "安装 bat（Homebrew 使用 `brew install bat`）",
        installer_asset: false,
    },
    ToolSpec {
        id: "fzf",
        probe: ProbeKind::OnPath,
        brew_package: None,
        version_range: VersionRange::Unrestricted,
        capability: "模糊搜索配色",
        remedy: "安装 fzf（Homebrew 使用 `brew install fzf`）",
        installer_asset: false,
    },
    ToolSpec {
        id: "zoxide",
        probe: ProbeKind::OnPath,
        brew_package: None,
        version_range: VersionRange::Unrestricted,
        capability: "目录跳转",
        remedy: "安装 zoxide（Homebrew 使用 `brew install zoxide`）",
        installer_asset: false,
    },
    ToolSpec {
        id: "zsh-syntax-highlighting",
        probe: ProbeKind::KnownPathsOrVendored(SYNTAX_HIGHLIGHTING_PATHS),
        brew_package: Some("zsh-syntax-highlighting"),
        version_range: VersionRange::Unrestricted,
        capability: "Zsh 语法高亮",
        remedy: "运行 `kiseki init`",
        installer_asset: false,
    },
    ToolSpec {
        id: "zsh-autosuggestions",
        probe: ProbeKind::KnownPathsOrVendored(AUTOSUGGESTIONS_PATHS),
        brew_package: Some("zsh-autosuggestions"),
        version_range: VersionRange::Unrestricted,
        capability: "Zsh 自动建议",
        remedy: "运行 `kiseki init`",
        installer_asset: false,
    },
];

#[derive(Debug)]
pub struct FontSpec {
    pub id: &'static str,
    pub repository: &'static str,
    pub source: FontSource,
    pub target_dirs: &'static FontTargetDirs,
}

#[derive(Debug)]
pub enum FontSource {
    ReleaseZip {
        asset_pattern: &'static str,
        filename_pattern: &'static str,
    },
    RawFiles {
        tag: &'static str,
        directory: &'static str,
        files: &'static [&'static str],
    },
}

#[derive(Debug)]
pub struct FontTargetDirs {
    pub macos: &'static str,
    pub linux: &'static str,
    pub windows: &'static str,
}

pub const FONT_TARGET_DIRS: FontTargetDirs = FontTargetDirs {
    macos: "~/Library/Fonts",
    linux: "~/.local/share/fonts/kiseki",
    windows: r"%LOCALAPPDATA%\Microsoft\Windows\Fonts",
};

pub const FONTS: &[FontSpec] = &[
    FontSpec {
        id: "jetbrains-mono-nf",
        repository: "ryanoasis/nerd-fonts",
        source: FontSource::RawFiles {
            tag: JETBRAINS_MONO_NF_TAG,
            directory: "patched-fonts/JetBrainsMono/Ligatures",
            files: &[
                "JetBrainsMonoNerdFont-Regular.ttf",
                "JetBrainsMonoNerdFont-Bold.ttf",
                "JetBrainsMonoNerdFont-Italic.ttf",
            ],
        },
        target_dirs: &FONT_TARGET_DIRS,
    },
    FontSpec {
        id: "departure-mono-nf",
        repository: "ryanoasis/nerd-fonts",
        source: FontSource::ReleaseZip {
            asset_pattern: r"^DepartureMono\.zip$",
            filename_pattern: r"\.(otf|ttf)$",
        },
        target_dirs: &FONT_TARGET_DIRS,
    },
    FontSpec {
        id: "maple-mono-nf-cn",
        repository: "subframe7536/maple-font",
        source: FontSource::ReleaseZip {
            asset_pattern: r"^MapleMono-NF-CN\.zip$",
            filename_pattern: r"-(Regular|Medium|Bold|Italic)\.ttf$",
        },
        target_dirs: &FONT_TARGET_DIRS,
    },
    FontSpec {
        id: "fusion-pixel-12-zh",
        repository: "TakWolf/fusion-pixel-font",
        source: FontSource::ReleaseZip {
            asset_pattern: r"12px-monospaced-ttf(?:-[^/]+)?\.zip$",
            filename_pattern: r"(zh_hans|ja)\.ttf$",
        },
        target_dirs: &FONT_TARGET_DIRS,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    System,
    Vendored,
    Missing,
}

impl ToolStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Vendored => "vendored",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ToolProbe {
    pub tool_id: &'static str,
    pub status: ToolStatus,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub remedy: String,
    pub capability: &'static str,
    pub compatible: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub ready: usize,
    pub missing: usize,
    pub incompatible: usize,
}

impl Summary {
    pub fn from_probes(probes: &[ToolProbe]) -> Self {
        let missing = probes
            .iter()
            .filter(|tool| tool.status == ToolStatus::Missing)
            .count();
        let incompatible = probes
            .iter()
            .filter(|tool| tool.compatible == Some(false))
            .count();
        Self {
            ready: probes.len() - missing - incompatible,
            missing,
            incompatible,
        }
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} 项已就绪 · {} 项缺失 · {} 项版本不兼容",
            self.ready, self.missing, self.incompatible
        )
    }
}

pub fn probe_all(platform: &dyn Platform) -> Vec<ToolProbe> {
    probe_all_cached(platform, false, |_, _| None, |_, _, _| {})
}

/// Probes every tool in [`TOOLS`], allowing callers to cache `--version`
/// results by binary path/metadata and skip version subprocesses for
/// unconstrained tools when only [`Summary`] is needed.
pub fn probe_all_cached(
    platform: &dyn Platform,
    summary_only: bool,
    mut lookup_version: impl FnMut(&ToolSpec, &Path) -> Option<Option<String>>,
    mut store_version: impl FnMut(&ToolSpec, &Path, Option<&str>),
) -> Vec<ToolProbe> {
    TOOLS
        .iter()
        .map(|tool| {
            probe_with(
                platform,
                tool,
                summary_only,
                &mut lookup_version,
                &mut store_version,
            )
        })
        .collect()
}

pub fn installer_bin(platform: &dyn Platform) -> PathBuf {
    platform.paths().vendor_bin(
        platform.os(),
        platform.env_var("KISEKI_INSTALL_DIR").as_deref(),
        platform.env_var("LOCALAPPDATA").as_deref(),
    )
}

pub fn installed_binary_path(platform: &dyn Platform, tool_id: &str) -> PathBuf {
    let filename = if platform.os() == OsKind::Windows {
        format!("{tool_id}.exe")
    } else {
        tool_id.to_owned()
    };
    installer_bin(platform).join(filename)
}

/// Resolves the executable used for a command that can be provisioned by the installer.
/// The private directory is never added to PATH.
pub fn executable(platform: &dyn Platform, tool_id: &str) -> Option<PathBuf> {
    let tool = TOOLS
        .iter()
        .find(|tool| tool.id == tool_id && tool.probe == ProbeKind::OnPath)?;
    locate(platform, tool).map(|(_, path)| path)
}

fn locate(platform: &dyn Platform, tool: &ToolSpec) -> Option<(ToolStatus, PathBuf)> {
    let (system, vendor) = match tool.probe {
        ProbeKind::OnPath => {
            let system = platform.which(tool.id);
            let vendor = tool
                .installer_asset
                .then(|| installed_binary_path(platform, tool.id));
            (system, vendor)
        }
        ProbeKind::KnownPaths(paths) | ProbeKind::KnownPathsOrVendored(paths) => {
            let system = paths
                .iter()
                .filter(|path| known_path_applicable(platform.os(), path))
                .map(Path::new)
                .find(|path| platform.exists(path))
                .map(Path::to_path_buf);
            let vendor = matches!(tool.probe, ProbeKind::KnownPathsOrVendored(_)).then(|| {
                platform
                    .paths()
                    .vendor_zsh()
                    .join(tool.id)
                    .join(format!("{}.zsh", tool.id))
            });
            (system, vendor)
        }
    };
    system.map(|path| (ToolStatus::System, path)).or_else(|| {
        vendor
            .filter(|path| platform.exists(path))
            .map(|path| (ToolStatus::Vendored, path))
    })
}

fn known_path_applicable(os: OsKind, path: &str) -> bool {
    match os {
        // On macOS `/home` triggers an `automountd` kernel RPC (~15 ms/miss).
        OsKind::MacOs => !path.starts_with("/home/"),
        OsKind::Windows => !path.starts_with('/'),
        OsKind::Linux => true,
    }
}

#[cfg(test)]
fn probe(platform: &dyn Platform, tool: &ToolSpec) -> ToolProbe {
    probe_with(platform, tool, false, &mut |_, _| None, &mut |_, _, _| {})
}

fn probe_with(
    platform: &dyn Platform,
    tool: &ToolSpec,
    summary_only: bool,
    lookup_version: &mut dyn FnMut(&ToolSpec, &Path) -> Option<Option<String>>,
    store_version: &mut dyn FnMut(&ToolSpec, &Path, Option<&str>),
) -> ToolProbe {
    let found = locate(platform, tool);
    let need_version = tool.probe == ProbeKind::OnPath
        && (!summary_only || matches!(tool.version_range, VersionRange::Major(_)));
    let version = if need_version {
        found.as_ref().and_then(|(_, path)| {
            if let Some(cached) = lookup_version(tool, path) {
                return cached;
            }
            let parsed = path
                .to_str()
                .and_then(|path_str| platform.run(path_str, &["--version"]).ok())
                .filter(|output| output.success())
                .and_then(|output| {
                    output
                        .stdout
                        .split_whitespace()
                        .chain(output.stderr.split_whitespace())
                        .find_map(|word| {
                            let word = word.strip_prefix('v').unwrap_or(word);
                            version_parts(word).map(|_| word.to_string())
                        })
                });
            store_version(tool, path, parsed.as_deref());
            parsed
        })
    } else {
        None
    };
    let compatible = match tool.version_range {
        VersionRange::Major(_) => version
            .as_deref()
            .map(|version| tool.version_range.accepts_binary(version)),
        _ => None,
    };
    let (status, path) = found.map_or((ToolStatus::Missing, None), |(status, path)| {
        (status, Some(path))
    });
    let remedy = if compatible == Some(false) && status == ToolStatus::System {
        "安装兼容的 fastfetch 2.x（PATH 中的版本优先）".to_string()
    } else if tool.installer_asset {
        format!(
            "{}，或运行 `brew install {}`",
            tool.remedy,
            tool.brew_package.unwrap_or(tool.id)
        )
    } else {
        tool.remedy.to_string()
    };
    ToolProbe {
        tool_id: tool.id,
        status,
        version,
        path,
        remedy,
        capability: tool.capability,
        compatible,
    }
}

fn version_parts(version: &str) -> Option<[u64; 3]> {
    let release = version.split(['-', '+']).next()?;
    let mut components = release.split('.');
    let mut parts = [0; 3];
    for part in &mut parts {
        let component = components.next()?;
        if component.len() > 1 && component.starts_with('0') {
            return None;
        }
        *part = component.parse().ok()?;
    }
    components.next().is_none().then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandOutput, MockPlatform};

    #[test]
    fn probes_system_vendored_and_missing_tools() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::new(dir.path());
        platform
            .set_os(OsKind::Linux)
            .set_env("KISEKI_WHICH_fastfetch", "/mock/fastfetch")
            .set_command_output(
                "/mock/fastfetch",
                CommandOutput::new(0, "fastfetch 3.0.0 (aarch64)", ""),
            );
        let vendor = platform
            .paths()
            .vendor_zsh()
            .join("zsh-autosuggestions/zsh-autosuggestions.zsh");
        platform
            .write_preserving_symlinks(&vendor, b"# vendored")
            .unwrap();
        let probes = probe_all(&platform);
        let find = |id| probes.iter().find(|tool| tool.tool_id == id).unwrap();
        assert_eq!(probes.len(), 8);
        assert_eq!(find("fastfetch").status, ToolStatus::System);
        assert_eq!(find("fastfetch").version.as_deref(), Some("3.0.0"));
        assert_eq!(find("fastfetch").compatible, Some(false));
        assert_eq!(
            find("zsh-autosuggestions").path.as_deref(),
            Some(vendor.as_path())
        );
        assert_eq!(find("fzf").status, ToolStatus::Missing);
        assert_eq!(find("zsh-autosuggestions").status, ToolStatus::Vendored);
        let known = Box::leak(Box::new([
            vendor.to_str().unwrap().to_owned().leak() as &str
        ]));
        let spec = ToolSpec {
            probe: ProbeKind::KnownPaths(known),
            ..TOOLS[0]
        };
        assert_eq!(probe(&platform, &spec).status, ToolStatus::System);
    }

    #[test]
    fn fastfetch_range_accepts_stable_two_series() {
        let range = VersionRange::Major(2);
        for version in ["2.0.0", "2.71.0", "2.999.999+build.1"] {
            assert!(range.accepts_binary(version), "{version}");
        }
        for version in ["1.99.0", "2.0.0-rc.1", "3.0.0"] {
            assert!(!range.accepts_binary(version), "{version}");
        }
    }

    #[test]
    fn linuxbrew_known_paths_are_skipped_on_macos() {
        use std::sync::Mutex;

        struct ProbeRecorder {
            inner: MockPlatform,
            probed: Mutex<Vec<PathBuf>>,
        }

        impl Platform for ProbeRecorder {
            fn os(&self) -> OsKind {
                self.inner.os()
            }
            fn paths(&self) -> &crate::KisekiPaths {
                self.inner.paths()
            }
            fn env_var(&self, key: &str) -> Option<String> {
                self.inner.env_var(key)
            }
            fn read_to_string(&self, path: &Path) -> kiseki_types::Result<Option<String>> {
                self.inner.read_to_string(path)
            }
            fn read_bytes(&self, path: &Path) -> kiseki_types::Result<Option<Vec<u8>>> {
                self.inner.read_bytes(path)
            }
            fn inspect(&self, path: &Path) -> crate::PathKind {
                self.inner.inspect(path)
            }
            fn write_preserving_symlinks(
                &self,
                path: &Path,
                contents: &[u8],
            ) -> kiseki_types::Result<crate::WriteOutcome> {
                self.inner.write_preserving_symlinks(path, contents)
            }
            fn remove_file(&self, path: &Path) -> kiseki_types::Result<()> {
                self.inner.remove_file(path)
            }
            fn create_dir_all(&self, path: &Path) -> kiseki_types::Result<()> {
                self.inner.create_dir_all(path)
            }
            fn list_dir(&self, path: &Path) -> kiseki_types::Result<Vec<PathBuf>> {
                self.inner.list_dir(path)
            }
            fn exists(&self, path: &Path) -> bool {
                self.probed.lock().unwrap().push(path.to_path_buf());
                false
            }
            fn run(&self, program: &str, args: &[&str]) -> kiseki_types::Result<CommandOutput> {
                self.inner.run(program, args)
            }
            fn which(&self, program: &str) -> Option<PathBuf> {
                self.inner.which(program)
            }
            fn process_alive(&self, pid: u32) -> bool {
                self.inner.process_alive(pid)
            }
            fn now_rfc3339(&self) -> String {
                self.inner.now_rfc3339()
            }
        }

        const KNOWN: &[&str] = &[
            "/opt/mockbrew/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh",
            "/home/linuxbrew/.linuxbrew/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh",
        ];
        let spec = ToolSpec {
            probe: ProbeKind::KnownPaths(KNOWN),
            ..TOOLS[6]
        };

        let dir = tempfile::tempdir().unwrap();
        let inner = MockPlatform::new(dir.path());
        inner.set_os(OsKind::MacOs);
        let mac_rec = ProbeRecorder {
            inner,
            probed: Mutex::new(Vec::new()),
        };
        let _ = probe(&mac_rec, &spec);
        let mac_probed = mac_rec.probed.lock().unwrap();
        assert!(!mac_probed.iter().any(|p| p.starts_with("/home/linuxbrew")));

        let inner_linux = MockPlatform::new(dir.path());
        inner_linux.set_os(OsKind::Linux);
        let linux_rec = ProbeRecorder {
            inner: inner_linux,
            probed: Mutex::new(Vec::new()),
        };
        let _ = probe(&linux_rec, &spec);
        let linux_probed = linux_rec.probed.lock().unwrap();
        assert!(linux_probed.iter().any(|p| p.starts_with("/home/linuxbrew")));
    }
}
