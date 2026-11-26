#!/usr/bin/env node
/**
 * Render Homebrew Formula (`kiseki.rb`) and Scoop bucket manifest (`kiseki.json`)
 * directly from a release `checksums.txt` file.
 *
 * Usage:
 *   node scripts/release/render-manifests.mjs \
 *     --version 2.0.0-alpha.1 \
 *     --checksums dist/checksums.txt \
 *     --brew-out packaging/homebrew/kiseki.rb \
 *     --scoop-out bucket/kiseki.json \
 *     [--repo nerv-base/kiseki-terminal] \
 *     [--allow-partial]
 */

import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  const opts = {
    version: "",
    checksums: "",
    brewOut: "",
    scoopOut: "",
    repo: "nerv-base/kiseki-terminal",
    allowPartial: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--version") opts.version = (argv[++i] || "").replace(/^v/, "");
    else if (arg === "--checksums") opts.checksums = argv[++i] || "";
    else if (arg === "--brew-out") opts.brewOut = argv[++i] || "";
    else if (arg === "--scoop-out") opts.scoopOut = argv[++i] || "";
    else if (arg === "--repo") opts.repo = argv[++i] || opts.repo;
    else if (arg === "--allow-partial") opts.allowPartial = true;
    else throw new Error(`Unknown argument: ${arg}`);
  }
  if (!opts.version) throw new Error("Missing --version");
  if (!opts.checksums) throw new Error("Missing --checksums");
  return opts;
}

function parseChecksums(filePath) {
  const map = new Map();
  const content = fs.readFileSync(filePath, "utf8");
  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const match = line.match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/);
    if (match) {
      map.set(path.basename(match[2].trim()), match[1].toLowerCase());
    }
  }
  return map;
}

function renderHomebrewFormula({ version, repo, sums, allowPartial }) {
  const tag = `v${version}`;
  const baseUrl = `https://github.com/${repo}/releases/download/${tag}`;

  const files = {
    macArm: `kiseki-${tag}-aarch64-apple-darwin.tar.gz`,
    macIntel: `kiseki-${tag}-x86_64-apple-darwin.tar.gz`,
    linuxArm: `kiseki-${tag}-aarch64-unknown-linux-musl.tar.gz`,
    linuxIntel: `kiseki-${tag}-x86_64-unknown-linux-musl.tar.gz`,
  };

  const requireSum = (key) => {
    const val = sums.get(files[key]);
    if (!val && !allowPartial) {
      throw new Error(`Missing checksum for Homebrew asset: ${files[key]}`);
    }
    return val || null;
  };

  const macArmSha = requireSum("macArm");
  const macIntelSha = requireSum("macIntel");
  const linuxArmSha = requireSum("linuxArm");
  const linuxIntelSha = requireSum("linuxIntel");

  const blocks = [];

  if (macArmSha || macIntelSha) {
    const inner = [];
    if (macArmSha) {
      inner.push(
        `    on_arm do\n` +
          `      url "${baseUrl}/${files.macArm}"\n` +
          `      sha256 "${macArmSha}"\n` +
          `    end`
      );
    }
    if (macIntelSha) {
      inner.push(
        `    on_intel do\n` +
          `      url "${baseUrl}/${files.macIntel}"\n` +
          `      sha256 "${macIntelSha}"\n` +
          `    end`
      );
    }
    blocks.push(`  on_macos do\n${inner.join("\n\n")}\n  end`);
  }

  if (linuxArmSha || linuxIntelSha) {
    const inner = [];
    if (linuxArmSha) {
      inner.push(
        `    on_arm do\n` +
          `      url "${baseUrl}/${files.linuxArm}"\n` +
          `      sha256 "${linuxArmSha}"\n` +
          `    end`
      );
    }
    if (linuxIntelSha) {
      inner.push(
        `    on_intel do\n` +
          `      url "${baseUrl}/${files.linuxIntel}"\n` +
          `      sha256 "${linuxIntelSha}"\n` +
          `    end`
      );
    }
    blocks.push(`  on_linux do\n${inner.join("\n\n")}\n  end`);
  }

  return `# typed: false
# frozen_string_literal: true

class Kiseki < Formula
  desc "Universal anime-aesthetic terminal theming engine (Rust single binary)"
  homepage "https://kisekiterm.dev"
  version "${version}"
  license "MIT"

${blocks.join("\n\n")}

  def install
    bin.install "kiseki"
    generate_completions_from_executable(bin/"kiseki", "completions")
  end

  def caveats
    <<~EOS
      Run the interactive setup wizard to attach Kiseki to your terminal and shell:
        kiseki init

      Optional companion tools for full prompt, showcase, and command highlighting:
        brew install oh-my-posh fastfetch zsh-syntax-highlighting
    EOS
  end

  test do
    assert_match "kiseki", shell_output("#{bin}/kiseki --version")
  end
end
`;
}

function renderScoopManifest({ version, repo, sums, allowPartial }) {
  const tag = `v${version}`;
  const baseUrl = `https://github.com/${repo}/releases/download/${tag}`;
  const win64File = `kiseki-${tag}-x86_64-pc-windows-msvc.zip`;
  const winArmFile = `kiseki-${tag}-aarch64-pc-windows-msvc.zip`;

  const win64Sha = sums.get(win64File);
  const winArmSha = sums.get(winArmFile);

  if (!allowPartial && (!win64Sha || !winArmSha)) {
    const missing = [!win64Sha && win64File, !winArmSha && winArmFile].filter(Boolean);
    throw new Error(`Missing checksum for Scoop asset(s): ${missing.join(", ")}`);
  }

  const architecture = {};
  if (win64Sha) {
    architecture["64bit"] = {
      url: `${baseUrl}/${win64File}`,
      hash: win64Sha,
      extract_dir: `kiseki-${tag}-x86_64-pc-windows-msvc`,
    };
  }
  if (winArmSha) {
    architecture["arm64"] = {
      url: `${baseUrl}/${winArmFile}`,
      hash: winArmSha,
      extract_dir: `kiseki-${tag}-aarch64-pc-windows-msvc`,
    };
  }

  const manifest = {
    version,
    description: "Universal anime-aesthetic terminal theming engine (Rust single binary)",
    homepage: "https://kisekiterm.dev",
    license: "MIT",
    architecture,
    bin: "kiseki.exe",
    checkver: {
      github: `https://github.com/${repo}`,
    },
    autoupdate: {
      architecture: {
        "64bit": {
          url: `https://github.com/${repo}/releases/download/v$version/kiseki-v$version-x86_64-pc-windows-msvc.zip`,
          extract_dir: "kiseki-v$version-x86_64-pc-windows-msvc",
        },
        arm64: {
          url: `https://github.com/${repo}/releases/download/v$version/kiseki-v$version-aarch64-pc-windows-msvc.zip`,
          extract_dir: "kiseki-v$version-aarch64-pc-windows-msvc",
        },
      },
      hash: {
        url: "$baseurl/checksums.txt",
      },
    },
    notes: [
      "Run `kiseki init` to attach Kiseki to Windows Terminal and PowerShell.",
    ],
  };

  return JSON.stringify(manifest, null, 2) + "\n";
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const sums = parseChecksums(opts.checksums);

  if (opts.brewOut) {
    fs.mkdirSync(path.dirname(opts.brewOut), { recursive: true });
    fs.writeFileSync(
      opts.brewOut,
      renderHomebrewFormula({
        version: opts.version,
        repo: opts.repo,
        sums,
        allowPartial: opts.allowPartial,
      })
    );
    console.log(`Wrote Homebrew formula: ${opts.brewOut}`);
  }

  if (opts.scoopOut) {
    const win64 = sums.get(`kiseki-v${opts.version}-x86_64-pc-windows-msvc.zip`);
    const winArm = sums.get(`kiseki-v${opts.version}-aarch64-pc-windows-msvc.zip`);
    if (win64 || winArm || !opts.allowPartial) {
      fs.mkdirSync(path.dirname(opts.scoopOut), { recursive: true });
      fs.writeFileSync(
        opts.scoopOut,
        renderScoopManifest({
          version: opts.version,
          repo: opts.repo,
          sums,
          allowPartial: opts.allowPartial,
        })
      );
      console.log(`Wrote Scoop manifest: ${opts.scoopOut}`);
    } else {
      console.log("Skipped Scoop manifest (no Windows checksums in partial mode)");
    }
  }
}

main();
