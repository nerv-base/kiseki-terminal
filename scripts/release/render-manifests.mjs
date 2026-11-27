#!/usr/bin/env node
/**
 * Render the Homebrew formula (`kiseki.rb`) from a release `checksums.txt`.
 *
 * Usage:
 *   node scripts/release/render-manifests.mjs \
 *     --version 2.0.0-alpha.1 \
 *     --checksums dist/checksums.txt \
 *     --brew-out packaging/homebrew/kiseki.rb \
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
    repo: "nerv-base/kiseki-terminal",
    allowPartial: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--version") opts.version = (argv[++i] || "").replace(/^v/, "");
    else if (arg === "--checksums") opts.checksums = argv[++i] || "";
    else if (arg === "--brew-out") opts.brewOut = argv[++i] || "";
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

  const renderArchBlock = (archMacro, file, sha) =>
    `    ${archMacro} do\n` +
    `      url "https://dl.kisekiterm.dev/releases/${tag}/${file}"\n` +
    `      mirror "https://kiseki-dl.nerv-base.com/releases/${tag}/${file}"\n` +
    `      mirror "${baseUrl}/${file}"\n` +
    `      sha256 "${sha}"\n` +
    `    end`;

  const blocks = [];

  if (macArmSha || macIntelSha) {
    const inner = [];
    if (macArmSha) inner.push(renderArchBlock("on_arm", files.macArm, macArmSha));
    if (macIntelSha) inner.push(renderArchBlock("on_intel", files.macIntel, macIntelSha));
    blocks.push(`  on_macos do\n${inner.join("\n\n")}\n  end`);
  }

  if (linuxArmSha || linuxIntelSha) {
    const inner = [];
    if (linuxArmSha) inner.push(renderArchBlock("on_arm", files.linuxArm, linuxArmSha));
    if (linuxIntelSha) inner.push(renderArchBlock("on_intel", files.linuxIntel, linuxIntelSha));
    blocks.push(`  on_linux do\n${inner.join("\n\n")}\n  end`);
  }

  return `# typed: false
# frozen_string_literal: true

class Kiseki < Formula
  desc "Universal anime-aesthetic terminal theming engine (Rust single binary)"
  homepage "https://kisekiterm.dev"
  version "${version}"
  license "MIT"

  depends_on "oh-my-posh"
  depends_on "fastfetch"
  depends_on "zsh-syntax-highlighting"
  depends_on "zsh-autosuggestions"

${blocks.join("\n\n")}

  def install
    bin.install "kiseki"
    pkgshare.install "NOTICE", "LICENSE.zsh-syntax-highlighting", "LICENSE.zsh-autosuggestions"
    generate_completions_from_executable(bin/"kiseki", "completions")
  end

  def caveats
    <<~EOS
      Run the interactive setup wizard to attach Kiseki to your terminal and shell:
        kiseki init
    EOS
  end

  test do
    assert_match "kiseki", shell_output("#{bin}/kiseki --version")
  end
end
`;
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
}

main();
