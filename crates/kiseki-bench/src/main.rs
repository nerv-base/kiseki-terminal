//! `kiseki-bench` — in-process benchmark harness for the Kiseki v2 hot paths.
//!
//! Every scenario is measured in release mode with batching (many calls per
//! timed round), nine rounds per scenario, and the per-call cost derived from
//! each round. Nothing touches the user's real configuration: the harness
//! pins `KISEKI_HOME` to a scratch directory before any platform lookup.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kiseki_adapters::{AdapterRegistry, PlanContext, PlanOptions};
use kiseki_compiler::{compile, CompileInput, CompileOptions};
use kiseki_pkg::{bundled_theme, validate_theme_dir, ThemeLibrary};
use kiseki_platform::{capture_env, detect, KisekiPaths, MockPlatform, Platform, SystemPlatform};
use kiseki_schema::{ResolvedTheme, ThemePack};
use kiseki_shader::{validate_glsl, validate_hlsl};
use kiseki_state::{Config, State};
use kiseki_tx::{ApplyRequest, TransactionManager, TransactionOptions};
use kiseki_types::{Durability, EnvContext, RenderingProfile, ThemeId};

const ROUNDS: usize = 9;
const TARGET_ROUND: Duration = Duration::from_millis(120);

// ---------------------------------------------------------------- statistics

struct Stats {
    min_ns: f64,
    median_ns: f64,
    mean_ns: f64,
    max_ns: f64,
    p90_ns: f64,
    cv_pct: f64,
}

impl Stats {
    fn from_rounds(values: &[f64]) -> Self {
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = sorted.len() as f64;
        let mean = sorted.iter().sum::<f64>() / n;
        let variance = sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let stddev = variance.sqrt();
        Self {
            min_ns: sorted[0],
            median_ns: percentile(&sorted, 0.5),
            mean_ns: mean,
            max_ns: sorted[sorted.len() - 1],
            p90_ns: percentile(&sorted, 0.9),
            cv_pct: if mean > 0.0 {
                stddev / mean * 100.0
            } else {
                0.0
            },
        }
    }
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
    }
}

struct BenchResult {
    name: String,
    group: &'static str,
    iters: u64,
    stats: Stats,
}

fn bench<F, R>(group: &'static str, name: impl Into<String>, mut f: F) -> BenchResult
where
    F: FnMut() -> R,
{
    let name = name.into();
    // Three untimed calls establish the steady-state cost (the first one may
    // include lazy init or a cold page cache) and force any lazy setup.
    let mut once = u128::MAX;
    for _ in 0..3 {
        let probe = Instant::now();
        let value = f();
        once = once.min(probe.elapsed().as_nanos().max(1));
        drop(black_box(value));
    }

    let max_iters = if once < 500 { 2_000_000 } else { 100_000 };
    let iters = ((TARGET_ROUND.as_nanos() / once) as u64).clamp(1, max_iters);
    for _ in 0..(iters / 2).min(10_000) {
        drop(black_box(f()));
    }
    // Let any dirty-page writeback from a preceding durable-write scenario
    // drain, so a cheap scenario measured right after one is not penalised.
    std::thread::sleep(Duration::from_millis(80));

    let mut rounds = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        for _ in 0..iters {
            drop(black_box(f()));
        }
        let per_op = start.elapsed().as_nanos() as f64 / iters as f64;
        rounds.push(per_op);
    }
    BenchResult {
        name,
        group,
        iters,
        stats: Stats::from_rounds(&rounds),
    }
}

// ------------------------------------------------------------------ fixtures

struct Fixture {
    id: &'static str,
    pack: ThemePack,
    glsl: String,
    hlsl: String,
    logo: String,
    /// Raw JSON, for parse benchmarks.
    tokens_json: String,
    manifest_json: String,
}

fn load_fixtures(root: &Path) -> Vec<Fixture> {
    let library = ThemeLibrary::open(&SystemPlatform::detect(), &[]);
    let mut out = Vec::new();
    for theme in kiseki_pkg::bundled_themes() {
        let dir = root.join(theme.id);
        let loaded = library
            .load_path(&dir)
            .unwrap_or_else(|error| panic!("load fixture {}: {error}", dir.display()));
        out.push(Fixture {
            id: theme.id,
            pack: loaded.pack,
            glsl: loaded.glsl.unwrap_or_default(),
            hlsl: loaded.hlsl.unwrap_or_default(),
            logo: loaded.logo.unwrap_or_default(),
            tokens_json: theme.tokens_json().to_string(),
            manifest_json: theme.manifest_json().to_string(),
        });
    }
    out
}

fn profile_name(profile: RenderingProfile) -> &'static str {
    match profile {
        RenderingProfile::Static => "static",
        RenderingProfile::Eco => "eco",
        RenderingProfile::Balanced => "balanced",
        RenderingProfile::Ultra => "ultra",
        RenderingProfile::Accessibility => "accessibility",
    }
}

// ---------------------------------------------------------------------- main

fn main() {
    let mut args = std::env::args().skip(1);
    let mut json_out: Option<PathBuf> = None;
    let mut filter: Option<String> = None;
    let mut quick = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json_out = args.next().map(PathBuf::from),
            "--filter" => filter = args.next(),
            "--quick" => quick = true,
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let theme_root = root.join("themes");
    let scratch = std::env::var("KISEKI_BENCH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("target/bench-home"));
    std::fs::create_dir_all(&scratch).expect("create scratch home");
    // Isolate every platform lookup before the first `SystemPlatform::detect`.
    std::env::set_var("KISEKI_HOME", &scratch);
    std::env::set_var("HOME", scratch.join("fake-home"));
    std::fs::create_dir_all(scratch.join("fake-home")).ok();

    let platform = SystemPlatform::detect();
    let paths: KisekiPaths = platform.paths().clone();

    // ---- prepare the state/config fixtures used by the hot-path benches.
    let mut state = State::new(
        ThemeId::new("nerv-eva").expect("valid id"),
        "2025-01-01T00:00:00Z",
    );
    state
        .commit(&platform, &paths)
        .expect("seed isolated state");
    Config::default()
        .save(&platform, &paths)
        .expect("seed config");
    let env: EnvContext = detect(&platform, &capture_env());
    let registry = AdapterRegistry::milestone1();
    let fixtures = load_fixtures(&theme_root);

    // Pre-resolved theme + artifacts reused by adapter benches.
    let resolved_all: Vec<ResolvedTheme> = fixtures
        .iter()
        .map(|fixture| ResolvedTheme::from_pack(&fixture.pack, RenderingProfile::Balanced))
        .collect();
    let nerv = &fixtures[0];
    let nerv_resolved = &resolved_all[0];
    let nerv_artifacts = compile(
        &CompileInput::new(
            nerv_resolved,
            CompileOptions::new(&scratch, theme_root.join(nerv.id)),
        )
        .with_glsl(Some(&nerv.glsl))
        .with_hlsl(Some(&nerv.hlsl))
        .with_logo(Some(&nerv.logo)),
    )
    .expect("compile nerv-eva fixture")
    .artifacts;

    let options = PlanOptions::default();
    let plan_ctx = PlanContext {
        theme: nerv_resolved,
        artifacts: &nerv_artifacts,
        options: &options,
        env: &env,
        paths: &paths,
    };

    let mut results: Vec<BenchResult> = Vec::new();
    let mut run = |group: &'static str, name: String, f: &mut dyn FnMut()| {
        if let Some(needle) = &filter {
            if !name.contains(needle.as_str()) && !group.contains(needle.as_str()) {
                return;
            }
        }
        eprintln!("  · {group} / {name}");
        results.push(bench(group, name, f));
    };

    // ---------------------------------------------------------- L1 parsing
    for fixture in &fixtures {
        let tokens = fixture.tokens_json.clone();
        run(
            "parse",
            format!("tokens.json · {}", fixture.id),
            &mut || {
                let parsed: kiseki_schema::ThemeTokens =
                    serde_json::from_str(black_box(tokens.as_str())).expect("tokens parse");
                black_box(parsed);
            },
        );

        let manifest = fixture.manifest_json.clone();
        run(
            "parse",
            format!("manifest.json · {}", fixture.id),
            &mut || {
                let parsed: kiseki_schema::ThemeManifest =
                    serde_json::from_str(black_box(manifest.as_str())).expect("manifest parse");
                black_box(parsed);
            },
        );

        let id = fixture.id;
        run(
            "parse",
            format!("bundled pack (json+assets) · {id}"),
            &mut || {
                let theme = bundled_theme(black_box(id)).expect("bundled theme");
                let manifest: kiseki_schema::ThemeManifest =
                    serde_json::from_str(theme.manifest_json()).expect("manifest");
                let tokens: kiseki_schema::ThemeTokens =
                    serde_json::from_str(theme.tokens_json()).expect("tokens");
                black_box(ThemePack { manifest, tokens });
            },
        );
    }

    // -------------------------------------------------------- L2 resolution
    for fixture in &fixtures {
        let pack = &fixture.pack;
        run(
            "resolve",
            format!("L1→L2 balanced · {}", fixture.id),
            &mut || {
                black_box(ResolvedTheme::from_pack(
                    black_box(pack),
                    RenderingProfile::Balanced,
                ));
            },
        );
    }
    {
        let pack = &nerv.pack;
        for profile in [
            RenderingProfile::Static,
            RenderingProfile::Eco,
            RenderingProfile::Ultra,
            RenderingProfile::Accessibility,
        ] {
            run(
                "resolve",
                format!("L1→L2 {} · nerv-eva", profile_name(profile)),
                &mut || {
                    black_box(ResolvedTheme::from_pack(black_box(pack), profile));
                },
            );
        }
    }
    for (fixture, resolved) in fixtures.iter().zip(&resolved_all) {
        run(
            "resolve",
            format!("WCAG contrast report · {}", fixture.id),
            &mut || {
                black_box(black_box(resolved).contrast_report());
            },
        );
    }

    // ------------------------------------------------------------- shader
    for (fixture, resolved) in fixtures.iter().zip(&resolved_all) {
        if !fixture.glsl.is_empty() {
            run(
                "shader",
                format!("GLSL header+wrap · {}", fixture.id),
                &mut || {
                    black_box(kiseki_shader::compile_glsl_for(
                        black_box(resolved),
                        black_box(&fixture.glsl),
                    ));
                },
            );
            run(
                "shader",
                format!("GLSL naga validate · {}", fixture.id),
                &mut || {
                    black_box(validate_glsl(black_box(resolved), black_box(&fixture.glsl)));
                },
            );
        }
        if !fixture.hlsl.is_empty() {
            run(
                "shader",
                format!("HLSL naga validate · {}", fixture.id),
                &mut || {
                    black_box(validate_hlsl(black_box(resolved), black_box(&fixture.hlsl)));
                },
            );
        }
    }

    // ----------------------------------------------------------- compiler
    let mut artifact_rows = Vec::new();
    for (fixture, resolved) in fixtures.iter().zip(&resolved_all) {
        let theme_dir = theme_root.join(fixture.id);
        let output = compile(
            &CompileInput::new(resolved, CompileOptions::new(&scratch, &theme_dir))
                .with_glsl(Some(&fixture.glsl))
                .with_hlsl(Some(&fixture.hlsl))
                .with_logo(Some(&fixture.logo)),
        )
        .expect("compile fixture");
        let bytes: usize = output
            .artifacts
            .iter()
            .map(|(_, content)| content.len())
            .sum();
        artifact_rows.push((fixture.id, output.artifacts.len(), bytes));

        run(
            "compile",
            format!("artifacts balanced · {}", fixture.id),
            &mut || {
                let output = compile(
                    &CompileInput::new(
                        black_box(resolved),
                        CompileOptions::new(&scratch, &theme_dir),
                    )
                    .with_glsl(Some(black_box(&fixture.glsl)))
                    .with_hlsl(Some(black_box(&fixture.hlsl)))
                    .with_logo(Some(black_box(&fixture.logo))),
                )
                .expect("compile");
                black_box(output);
            },
        );
    }
    {
        let theme_dir = theme_root.join(nerv.id);
        let static_resolved = ResolvedTheme::from_pack(&nerv.pack, RenderingProfile::Static);
        run(
            "compile",
            "artifacts static · nerv-eva".to_string(),
            &mut || {
                let output = compile(
                    &CompileInput::new(&static_resolved, CompileOptions::new(&scratch, &theme_dir))
                        .with_logo(Some(&nerv.logo)),
                )
                .expect("compile");
                black_box(output);
            },
        );
    }

    // ------------------------------------------------------------ adapters
    {
        let ghostty = registry.by_id("ghostty").expect("ghostty adapter");
        run("adapters", "ghostty detect".to_string(), &mut || {
            black_box(ghostty.detect(black_box(&env), &platform).expect("detect"));
        });
        let detection = ghostty.detect(&env, &platform).expect("detect");
        let caps = ghostty.capabilities(&detection, &env);
        run(
            "adapters",
            "ghostty plan (dry-run)".to_string(),
            &mut || {
                black_box(
                    ghostty
                        .plan(black_box(&plan_ctx), black_box(&detection), caps)
                        .expect("plan"),
                );
            },
        );
        run(
            "adapters",
            "registry detect_all (6 adapters)".to_string(),
            &mut || {
                black_box(registry.detect_all(black_box(&env), &platform, &options));
            },
        );
    }

    // ---------------------------------------------------------------- pkg
    run("pkg", "ThemeLibrary::open".to_string(), &mut || {
        black_box(ThemeLibrary::open(&platform, &[]));
    });
    {
        let library = ThemeLibrary::open(&platform, &[]);
        run("pkg", "library list (4 official)".to_string(), &mut || {
            black_box(library.list(&platform).expect("list"));
        });
        for fixture in &fixtures {
            let id = ThemeId::new(fixture.id).expect("id");
            run(
                "pkg",
                format!("load_readonly (dry-run) · {}", fixture.id),
                &mut || {
                    black_box(
                        library
                            .load_readonly(&platform, black_box(&id))
                            .expect("load_readonly"),
                    );
                },
            );
        }
        // Materialise once so the steady-state `load` path hits plain disk reads.
        for fixture in &fixtures {
            let id = ThemeId::new(fixture.id).expect("id");
            library.load(&platform, &id).expect("materialise");
        }
        for fixture in &fixtures {
            let id = ThemeId::new(fixture.id).expect("id");
            run(
                "pkg",
                format!("load (materialised) · {}", fixture.id),
                &mut || {
                    black_box(library.load(&platform, black_box(&id)).expect("load"));
                },
            );
        }
    }
    for fixture in &fixtures {
        let dir = theme_root.join(fixture.id);
        run(
            "pkg",
            format!("validate pack dir · {}", fixture.id),
            &mut || {
                black_box(validate_theme_dir(black_box(&dir)).expect("validate"));
            },
        );
    }

    // --------------------------------------------------------- state/config
    run("state", "State::load".to_string(), &mut || {
        black_box(State::load(&platform, &paths).expect("state load"));
    });
    {
        let seeded = State::load(&platform, &paths)
            .expect("state")
            .expect("seeded");
        run("state", "State::save".to_string(), &mut || {
            black_box(&seeded).save(&platform, &paths).expect("save");
            black_box(());
        });
    }
    run(
        "state",
        "State::read_gen (shell hot path)".to_string(),
        &mut || {
            black_box(State::read_gen(&platform, &paths).expect("gen"));
        },
    );
    run("state", "Config::load".to_string(), &mut || {
        black_box(Config::load(&platform, &paths).expect("config"));
    });
    run(
        "state",
        "env detect (capture_env+detect)".to_string(),
        &mut || {
            black_box(detect(&platform, &capture_env()));
        },
    );

    // ------------------------------------------------------- transactions
    {
        let tx_gen_root = scratch.join("tx-bench-gen");
        let _ = std::fs::remove_dir_all(&tx_gen_root);
        let gen_platform = MockPlatform::new(&tx_gen_root);
        let gen_manager = TransactionManager::new(&gen_platform, AdapterRegistry::milestone1());
        let kyoto = &fixtures[1];
        let kyoto_resolved = &resolved_all[1];
        let gen_nerv_artifacts = compile(
            &CompileInput::new(
                nerv_resolved,
                CompileOptions::new(gen_platform.paths().config_dir(), theme_root.join(nerv.id)),
            )
            .with_glsl(Some(&nerv.glsl))
            .with_hlsl(Some(&nerv.hlsl))
            .with_logo(Some(&nerv.logo)),
        )
        .expect("compile nerv")
        .artifacts;
        let gen_kyoto_artifacts = compile(
            &CompileInput::new(
                kyoto_resolved,
                CompileOptions::new(gen_platform.paths().config_dir(), theme_root.join(kyoto.id)),
            )
            .with_glsl(Some(&kyoto.glsl))
            .with_hlsl(Some(&kyoto.hlsl))
            .with_logo(Some(&kyoto.logo)),
        )
        .expect("compile kyoto")
        .artifacts;
        let mut gen_state = State::new(nerv_resolved.id.clone(), "2025-01-01T00:00:00Z");
        gen_state = gen_manager
            .apply(ApplyRequest {
                theme: nerv_resolved,
                artifacts: &gen_nerv_artifacts,
                state_before: gen_state.clone(),
                state_after: gen_state.clone(),
                env: &env,
                options: TransactionOptions {
                    render_only: true,
                    durability: Durability::Batch,
                    ..TransactionOptions::default()
                },
            })
            .expect("seed tx gen")
            .state;
        let mut toggle_gen = false;
        run("tx", "tx apply (generated only)".to_string(), &mut || {
            toggle_gen = !toggle_gen;
            let (theme, artifacts) = if toggle_gen {
                (kyoto_resolved, &gen_kyoto_artifacts)
            } else {
                (nerv_resolved, &gen_nerv_artifacts)
            };
            let mut next = gen_state.clone();
            next.active_theme = theme.id.clone();
            let report = gen_manager
                .apply(ApplyRequest {
                    theme,
                    artifacts,
                    state_before: gen_state.clone(),
                    state_after: next,
                    env: &env,
                    options: TransactionOptions {
                        render_only: true,
                        durability: Durability::Batch,
                        ..TransactionOptions::default()
                    },
                })
                .expect("tx apply generated");
            gen_state = report.state;
            black_box(gen_state.generation);
        });

        let tx_full_root = scratch.join("tx-bench-full");
        let _ = std::fs::remove_dir_all(&tx_full_root);
        std::fs::create_dir_all(&tx_full_root).expect("create tx full root");
        let full_platform = MockPlatform::new(&tx_full_root);
        let zshrc = tx_full_root.join(".zshrc");
        let ghostty = tx_full_root.join("ghostty-config");
        std::fs::write(&zshrc, "# bench zshrc\n").expect("write zshrc");
        std::fs::write(&ghostty, "# bench ghostty\n").expect("write ghostty");
        full_platform.set_env("KISEKI_ZSHRC", &zshrc.to_string_lossy());
        full_platform.set_env("KISEKI_GHOSTTY_CONFIG", &ghostty.to_string_lossy());
        let full_manager = TransactionManager::new(&full_platform, AdapterRegistry::milestone1());
        let full_nerv_artifacts = compile(
            &CompileInput::new(
                nerv_resolved,
                CompileOptions::new(full_platform.paths().config_dir(), theme_root.join(nerv.id)),
            )
            .with_glsl(Some(&nerv.glsl))
            .with_hlsl(Some(&nerv.hlsl))
            .with_logo(Some(&nerv.logo)),
        )
        .expect("compile nerv")
        .artifacts;
        let full_kyoto_artifacts = compile(
            &CompileInput::new(
                kyoto_resolved,
                CompileOptions::new(full_platform.paths().config_dir(), theme_root.join(kyoto.id)),
            )
            .with_glsl(Some(&kyoto.glsl))
            .with_hlsl(Some(&kyoto.hlsl))
            .with_logo(Some(&kyoto.logo)),
        )
        .expect("compile kyoto")
        .artifacts;
        let full_opts = || TransactionOptions {
            only: vec!["ghostty".to_string(), "zsh".to_string()],
            explicit_only: true,
            durability: Durability::Batch,
            ..TransactionOptions::default()
        };
        let mut full_state = State::new(nerv_resolved.id.clone(), "2025-01-01T00:00:00Z");
        full_state = full_manager
            .apply(ApplyRequest {
                theme: nerv_resolved,
                artifacts: &full_nerv_artifacts,
                state_before: full_state.clone(),
                state_after: full_state.clone(),
                env: &env,
                options: full_opts(),
            })
            .expect("seed tx full")
            .state;
        let mut toggle_full = false;
        run("tx", "tx apply (full)".to_string(), &mut || {
            toggle_full = !toggle_full;
            let (theme, artifacts) = if toggle_full {
                (kyoto_resolved, &full_kyoto_artifacts)
            } else {
                (nerv_resolved, &full_nerv_artifacts)
            };
            let mut next = full_state.clone();
            next.active_theme = theme.id.clone();
            let report = full_manager
                .apply(ApplyRequest {
                    theme,
                    artifacts,
                    state_before: full_state.clone(),
                    state_after: next,
                    env: &env,
                    options: full_opts(),
                })
                .expect("tx apply full");
            full_state = report.state;
            black_box(full_state.generation);
        });
    }

    // ------------------------------------------------------------ io floor
    {
        let io_dir = scratch.join("io-bench");
        std::fs::create_dir_all(&io_dir).expect("io dir");
        let payload = vec![b'x'; 4096];
        let durable_path = io_dir.join("durable.conf");
        let plain_path = io_dir.join("plain.conf");

        run("io", "write 4 KiB (no fsync)".to_string(), &mut || {
            std::fs::write(black_box(&plain_path), black_box(&payload)).expect("plain write");
        });
        run("io", "write 4 KiB (fsync only)".to_string(), &mut || {
            use std::io::Write as _;
            let mut file = std::fs::File::create(black_box(&plain_path)).expect("create");
            file.write_all(black_box(&payload)).expect("write");
            file.sync_data().expect("fsync");
        });
        run(
            "io",
            "write 4 KiB (sync_all, F_FULLFSYNC)".to_string(),
            &mut || {
                use std::io::Write as _;
                let mut file = std::fs::File::create(black_box(&plain_path)).expect("create");
                file.write_all(black_box(&payload)).expect("write");
                file.sync_all().expect("full fsync");
            },
        );
        run(
            "io",
            "durable atomic write (platform)".to_string(),
            &mut || {
                kiseki_platform::fs::atomic_write(black_box(&durable_path), black_box(&payload))
                    .expect("atomic write");
            },
        );
    }

    // --------------------------------------------------------------- report
    println!();
    println!(
        "{:<10} {:<44} {:>10} {:>11} {:>11} {:>9} {:>11}",
        "group", "scenario", "iters", "median", "mean", "cv%", "ops/s"
    );
    println!("{}", "-".repeat(102));
    let mut last_group = "";
    for result in &results {
        if result.group != last_group {
            last_group = result.group;
        }
        println!(
            "{:<10} {:<44} {:>10} {:>11} {:>11} {:>8.1}% {:>11}",
            result.group,
            truncate(&result.name, 44),
            result.iters,
            human(result.stats.median_ns),
            human(result.stats.mean_ns),
            result.stats.cv_pct,
            ops_per_sec(result.stats.median_ns),
        );
    }
    println!();
    println!("artifact bytes per theme:");
    for (id, count, bytes) in &artifact_rows {
        println!("  {id:<16} {count:>3} artifacts  {:>8} bytes", bytes);
    }
    if quick {
        eprintln!("(--quick is accepted for compatibility; the adaptive scheduler already keeps runs short)");
    }

    if let Some(path) = json_out {
        let documents: Vec<serde_json::Value> = results
            .iter()
            .map(|result| {
                serde_json::json!({
                    "group": result.group,
                    "scenario": result.name,
                    "iters_per_round": result.iters,
                    "rounds": ROUNDS,
                    "median_ns": result.stats.median_ns,
                    "mean_ns": result.stats.mean_ns,
                    "min_ns": result.stats.min_ns,
                    "max_ns": result.stats.max_ns,
                    "p90_ns": result.stats.p90_ns,
                    "cv_pct": result.stats.cv_pct,
                    "ops_per_sec": 1e9 / result.stats.median_ns,
                })
            })
            .collect();
        let doc = serde_json::json!({
            "meta": {
                "profile": "release",
                "rounds": ROUNDS,
                "target_round_ms": TARGET_ROUND.as_millis(),
                "note": result_note(),
                "artifacts": artifact_rows,
            },
            "results": documents,
        });
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, serde_json::to_string_pretty(&doc).expect("json"))
            .expect("write bench json");
        eprintln!("\nwrote {}", path.display());
    }
}

fn result_note() -> String {
    format!(
        "{} scenarios; adaptive batching; medians over {ROUNDS} rounds; isolated KISEKI_HOME",
        "kiseki-v2"
    )
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    let mut out: String = value.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn human(ns: f64) -> String {
    if ns >= 1_000_000.0 {
        format!("{:.3} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.2} µs", ns / 1_000.0)
    } else {
        format!("{ns:.0} ns")
    }
}

fn ops_per_sec(ns: f64) -> String {
    let ops = 1e9 / ns.max(1.0);
    if ops >= 1_000_000.0 {
        format!("{:.2}M", ops / 1_000_000.0)
    } else if ops >= 1_000.0 {
        format!("{:.1}k", ops / 1_000.0)
    } else {
        format!("{ops:.0}")
    }
}

#[allow(dead_code)]
fn parse_theme_pack(manifest: &str, tokens: &str) -> Option<ThemePack> {
    Some(ThemePack {
        manifest: serde_json::from_str(manifest).ok()?,
        tokens: serde_json::from_str(tokens).ok()?,
    })
}
