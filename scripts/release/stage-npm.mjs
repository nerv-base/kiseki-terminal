#!/usr/bin/env node
/**
 * Stage the main `kiseki-terminal` npm/bun package and its 6 platform binary
 * optionalDependency sub-packages for local verification or CI publishing.
 *
 * Usage:
 *   node scripts/release/stage-npm.mjs \
 *     --version 2.0.0-alpha.1 \
 *     --binaries-dir dist/binaries \
 *     --out-dir dist/npm \
 *     [--allow-partial]
 *
 * Expected binary locations inside --binaries-dir (first match wins):
 *   1. <binaries-dir>/<rust-target>/<binary>
 *   2. <binaries-dir>/kiseki-v<version>-<rust-target>/<binary>
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..", "..");
const TEMPLATE_DIR = path.join(REPO_ROOT, "npm", "kiseki-terminal");

export const TARGETS = [
  {
    rustTarget: "aarch64-apple-darwin",
    pkgName: "kiseki-terminal-darwin-arm64",
    os: "darwin",
    cpu: "arm64",
    binName: "kiseki",
  },
  {
    rustTarget: "x86_64-apple-darwin",
    pkgName: "kiseki-terminal-darwin-x64",
    os: "darwin",
    cpu: "x64",
    binName: "kiseki",
  },
  {
    rustTarget: "aarch64-unknown-linux-musl",
    pkgName: "kiseki-terminal-linux-arm64",
    os: "linux",
    cpu: "arm64",
    binName: "kiseki",
  },
  {
    rustTarget: "x86_64-unknown-linux-musl",
    pkgName: "kiseki-terminal-linux-x64",
    os: "linux",
    cpu: "x64",
    binName: "kiseki",
  },
  {
    rustTarget: "aarch64-pc-windows-msvc",
    pkgName: "kiseki-terminal-win32-arm64",
    os: "win32",
    cpu: "arm64",
    binName: "kiseki.exe",
  },
  {
    rustTarget: "x86_64-pc-windows-msvc",
    pkgName: "kiseki-terminal-win32-x64",
    os: "win32",
    cpu: "x64",
    binName: "kiseki.exe",
  },
];

function parseArgs(argv) {
  const opts = { version: "", binariesDir: "", outDir: "", allowPartial: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--version") opts.version = (argv[++i] || "").replace(/^v/, "");
    else if (arg === "--binaries-dir") opts.binariesDir = argv[++i] || "";
    else if (arg === "--out-dir") opts.outDir = argv[++i] || "";
    else if (arg === "--allow-partial") opts.allowPartial = true;
    else throw new Error(`Unknown argument: ${arg}`);
  }
  if (!opts.version) throw new Error("Missing required --version <semver>");
  if (!opts.binariesDir) throw new Error("Missing required --binaries-dir <dir>");
  if (!opts.outDir) throw new Error("Missing required --out-dir <dir>");
  return opts;
}

function findBinary(binariesDir, version, target) {
  const candidates = [
    path.join(binariesDir, target.rustTarget, target.binName),
    path.join(binariesDir, `kiseki-v${version}-${target.rustTarget}`, target.binName),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  return null;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const outDir = path.resolve(opts.outDir);
  const binariesDir = path.resolve(opts.binariesDir);

  fs.rmSync(outDir, { recursive: true, force: true });
  fs.mkdirSync(outDir, { recursive: true });

  const licensePath = path.join(REPO_ROOT, "LICENSE");
  const mainPkgTemplate = JSON.parse(
    fs.readFileSync(path.join(TEMPLATE_DIR, "package.json"), "utf8")
  );

  const stagedPackages = [];
  const missing = [];

  for (const target of TARGETS) {
    const srcBin = findBinary(binariesDir, opts.version, target);
    if (!srcBin) {
      missing.push(target.rustTarget);
      continue;
    }

    const pkgDir = path.join(outDir, target.pkgName);
    const binDir = path.join(pkgDir, "bin");
    fs.mkdirSync(binDir, { recursive: true });

    const destBin = path.join(binDir, target.binName);
    fs.copyFileSync(srcBin, destBin);
    if (target.os !== "win32") {
      fs.chmodSync(destBin, 0o755);
    }

    const subPkg = {
      name: target.pkgName,
      version: opts.version,
      description: `Prebuilt ${target.os}/${target.cpu} native binary for kiseki-terminal`,
      license: mainPkgTemplate.license,
      homepage: mainPkgTemplate.homepage,
      repository: {
        type: "git",
        url: "git+https://github.com/nerv-base/kiseki-terminal.git",
      },
      os: [target.os],
      cpu: [target.cpu],
      files: ["bin/", "README.md", "LICENSE"],
      preferUnplugged: true,
    };

    fs.writeFileSync(
      path.join(pkgDir, "package.json"),
      JSON.stringify(subPkg, null, 2) + "\n"
    );
    fs.writeFileSync(
      path.join(pkgDir, "README.md"),
      `# ${target.pkgName}\n\nPrebuilt \`${target.rustTarget}\` binary for [kiseki-terminal](https://github.com/nerv-base/kiseki-terminal). Install \`kiseki-terminal\` instead of this package directly.\n`
    );
    if (fs.existsSync(licensePath)) {
      fs.copyFileSync(licensePath, path.join(pkgDir, "LICENSE"));
    }

    stagedPackages.push({ name: target.pkgName, dir: pkgDir, target: target.rustTarget });
  }

  if (missing.length > 0 && !opts.allowPartial) {
    throw new Error(
      `Missing compiled binaries for targets: ${missing.join(", ")} (pass --allow-partial to override)`
    );
  }

  // Stage main wrapper package last (so publish order is platform sub-packages -> main package)
  const mainDir = path.join(outDir, "kiseki-terminal");
  fs.mkdirSync(path.join(mainDir, "bin"), { recursive: true });

  const optionalDependencies = {};
  for (const target of TARGETS) {
    optionalDependencies[target.pkgName] = opts.version;
  }

  const mainPkg = {
    ...mainPkgTemplate,
    version: opts.version,
    optionalDependencies,
  };

  fs.writeFileSync(
    path.join(mainDir, "package.json"),
    JSON.stringify(mainPkg, null, 2) + "\n"
  );
  const srcWrapper = path.join(TEMPLATE_DIR, "bin", "kiseki.js");
  const destWrapper = path.join(mainDir, "bin", "kiseki.js");
  fs.copyFileSync(srcWrapper, destWrapper);
  fs.chmodSync(destWrapper, 0o755);
  fs.copyFileSync(
    path.join(TEMPLATE_DIR, "README.md"),
    path.join(mainDir, "README.md")
  );
  if (fs.existsSync(licensePath)) {
    fs.copyFileSync(licensePath, path.join(mainDir, "LICENSE"));
  }

  stagedPackages.push({ name: "kiseki-terminal", dir: mainDir, target: "wrapper" });

  console.log(
    JSON.stringify(
      {
        version: opts.version,
        missing,
        packages: stagedPackages,
      },
      null,
      2
    )
  );
}

main();
