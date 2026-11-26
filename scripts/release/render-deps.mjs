#!/usr/bin/env node
/* Render the standalone installers' release asset manifest from deps.rs. */

import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { parseArgs } from "node:util";

const depsPath = new URL("../../crates/kiseki-platform/src/deps.rs", import.meta.url);
const { values: options } = parseArgs({ options: {
  out: { type: "string", default: "dist/deps.tsv" },
  check: { type: "string" },
  "metadata-only": { type: "boolean" },
  "update-pins": { type: "boolean" },
} });

const toolReleases = {
  "oh-my-posh": { repo: "JanDeDobbeleer/oh-my-posh", pin: "OMP_VERSION", tagPrefix: "v" },
  fastfetch: { repo: "fastfetch-cli/fastfetch", pin: "FASTFETCH_VERSION", tagPrefix: "" },
};

// Release assets differ even though the Kiseki Linux binary itself always targets musl.
const platforms = [
  ["darwin-arm64", "darwin-arm64", "macos-universal.zip"],
  ["darwin-amd64", "darwin-amd64", "macos-universal.zip"],
  ["linux-arm64", "linux-arm64", "linux-aarch64-polyfilled.zip"],
  ["linux-amd64", "linux-amd64", "linux-amd64-polyfilled.zip"],
  ["linux-musl-arm64", "linux-arm64", null], // No native upstream Fastfetch archive.
  ["linux-musl-amd64", "linux-amd64", "musl-amd64.zip"],
  ["windows-arm64", "windows-arm64.exe", "windows-aarch64.zip"],
  ["windows-amd64", "windows-amd64.exe", "windows-amd64.zip"],
];

const source = await readFile(depsPath, "utf8");

function constant(name) {
  return source.match(new RegExp(`^pub const ${name}: &str = "([^"]+)";$`, "m"))?.[1];
}

function table(name, type) {
  const records = source.match(new RegExp(`pub const ${name}: &\\[${type}\\] = &\\[([\\s\\S]*?)^\\];`, "m"))?.[1];
  if (!records) throw new Error(`Missing Rust table ${name}`);
  return [...records.matchAll(new RegExp(`${type} \\{([\\s\\S]*?)\\n    \\},`, "g"))].map(([, record]) => record);
}

function field(record, name) {
  const value = record.match(new RegExp(`\\b${name}: r?"([^"]+)"`))?.[1];
  if (!value) throw new Error(`Missing Rust field ${name} in ${record}`);
  return value;
}

const installerTools = table("TOOLS", "ToolSpec")
  .filter((record) => /\binstaller_asset: true\b/.test(record))
  .map((record) => field(record, "id"));
const fonts = table("FONTS", "FontSpec").map((record) => {
  const id = field(record, "id");
  const repo = field(record, "repository");
  if (record.includes("source: FontSource::ReleaseZip {")) {
    return { id, repo, asset: new RegExp(field(record, "asset_pattern")), filename: field(record, "filename_pattern") };
  }
  if (record.includes("source: FontSource::RawFiles {")) {
    const pin = record.match(/\btag: ([A-Z][A-Z0-9_]+),/)?.[1];
    const tag = pin && constant(pin);
    const directory = field(record, "directory");
    const list = record.match(/\bfiles: &\[([\s\S]*?)\]/)?.[1];
    const files = [...(list ?? "").matchAll(/"([^"]+)"/g)].map(([, name]) => name);
    if (!tag || !/^v?\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(tag) || !files.length ||
        files.some((name) => !/^[\w-]+\.ttf$/.test(name))) {
      throw new Error(`Invalid raw font source for ${id}`);
    }
    return { id, repo, tag, directory, files };
  }
  throw new Error(`Unknown font source for ${id}`);
});
const fontDirs = source.match(/pub const FONT_TARGET_DIRS: FontTargetDirs = FontTargetDirs \{([\s\S]*?)\};/)?.[1];
const comments = [
  "# kind\tplatform\turl\tsha256\tdest\tmode",
  ...["macos", "linux", "windows"].map((os) => `# font-dir\t${os}\t${field(fontDirs, os)}`),
  ...fonts.filter((font) => font.filename).map((font) => `# font\t${font.id}\t${font.filename}`),
];

if (installerTools.length !== Object.keys(toolReleases).length || installerTools.some((id) => !toolReleases[id])) {
  throw new Error(`Installer tools in Rust differ from upstream mappings: ${installerTools.join(", ")}`);
}

const releases = new Map();
function release(repo, tag) {
  const path = tag ? `tags/${encodeURIComponent(tag)}` : "latest";
  const url = `https://api.github.com/repos/${repo}/releases/${path}`;
  if (!releases.has(url)) {
    const headers = { Accept: "application/vnd.github+json" };
    if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
    releases.set(url, download(url, headers).then((response) => response.json()));
  }
  return releases.get(url);
}

async function download(url, headers = {}) {
  const response = await fetch(url, { headers, signal: AbortSignal.timeout(900_000) });
  if (!response.ok) throw new Error(`${url} returned HTTP ${response.status}`);
  return response;
}

const checksums = new Map();
function checksum({ browser_download_url: url, digest: upstreamDigest, name: assetName }, releaseInfo) {
  if (!checksums.has(url)) {
    checksums.set(url, (async () => {
      const upstream = releaseInfo?.assets.find(({ name }) => name === "checksums.txt");
      if (upstream) {
        const body = await assetText(upstream);
        for (const line of body.split(/\r?\n/)) {
          const [, digest, name] = line.match(/^([0-9a-f]{64})\s+\*?(.+)$/i) ?? [];
          if (name === assetName) return digest.toLowerCase();
        }
      }
      // The quick CI path may use GitHub's release-asset checksum. The release
      // path streams assets that have no upstream checksum file.
      if (options["metadata-only"] && /^sha256:[0-9a-f]{64}$/.test(upstreamDigest)) return upstreamDigest.slice(7);
      try {
        const response = await download(url);
        const hash = createHash("sha256");
        for await (const chunk of response.body) hash.update(chunk);
        const digest = hash.digest("hex");
        if (upstreamDigest && `sha256:${digest}` !== upstreamDigest) throw new Error("GitHub release digest mismatch");
        return digest;
      } catch (error) {
        throw new Error(`Could not hash release asset ${url}`, { cause: error });
      }
    })());
  }
  return checksums.get(url);
}

const assetTexts = new Map();
function assetText(asset) {
  const url = asset.browser_download_url;
  if (!assetTexts.has(url)) assetTexts.set(url, download(url).then((response) => response.text()));
  return assetTexts.get(url);
}

function toolRows() {
  return platforms.flatMap(([platform, posh, fastfetch]) => installerTools.flatMap((id) => {
    const asset = id === "oh-my-posh" ? `posh-${posh}` : fastfetch && `fastfetch-${fastfetch}`;
    if (!asset) return [];
    const { repo, pin, tagPrefix } = toolReleases[id];
    const version = constant(pin);
    if (!version) throw new Error(`Missing version constant ${pin}`);
    const suffix = platform.startsWith("windows-") ? ".exe" : "";
    return [{ kind: "tool", platform, repo, tag: tagPrefix + version, asset, dest: `vendor/bin/${id}${suffix}`, mode: "0755" }];
  }));
}

function fontRows() {
  return platforms.flatMap(([platform]) => fonts.flatMap((font) => {
    if (font.files) {
      const base = `https://raw.githubusercontent.com/${font.repo}/${font.tag}/${font.directory}`;
      return font.files.map((name) => ({
        kind: "font-raw", platform, url: `${base}/${name}`, dest: `fonts/${name}`, mode: "0644",
      }));
    }
    return [{ kind: "font", platform, repo: font.repo, pattern: font.asset, dest: `fonts/${font.id}`, mode: "0644" }];
  }));
}

async function renderRow(row) {
  const { kind, platform, dest, mode } = row;
  if (row.url) return [kind, platform, row.url, await checksum({ browser_download_url: row.url }), dest, mode].join("\t");
  const info = await release(row.repo, row.tag);
  const matches = info.assets.filter(({ name }) => row.asset ? name === row.asset : row.pattern.test(name));
  if (matches.length !== 1) {
    throw new Error(`${row.repo} ${info.tag_name}: expected one asset for ${row.asset ?? row.pattern}, found ${matches.length}`);
  }
  const [asset] = matches;
  const sha = await checksum(asset, info);
  return [kind, platform, asset.browser_download_url, sha, dest, mode].join("\t");
}

function checkManifest(tsv, rows) {
  const expected = new Map(rows.map((row) => [[row.kind, row.platform, row.dest].join("\t"), row]));
  const lines = tsv.trimEnd().split(/\r?\n/);
  for (const comment of comments) {
    if (!lines.includes(comment)) throw new Error(`Missing manifest metadata: ${comment}`);
  }
  if (lines[0] !== comments[0]) throw new Error("Missing TSV header");
  for (const line of lines.filter((line) => !line.startsWith("#"))) {
    const fields = line.split("\t");
    const [kind, platform, url, sha, dest, mode] = fields;
    const key = [kind, platform, dest].join("\t");
    const row = expected.get(key);
    if (!row || fields.length !== 6 || mode !== row.mode || !/^[0-9a-f]{64}$/.test(sha)) {
      throw new Error(`Unexpected dependency row: ${line}`);
    }
    if (row.url) {
      if (url !== row.url) throw new Error(`Unexpected raw asset URL for ${dest}: ${url}`);
    } else {
      const prefix = `https://github.com/${row.repo}/releases/download/`;
      const [tag, asset, ...extra] = url.slice(prefix.length).split("/").map(decodeURIComponent);
      if (!url.startsWith(prefix) || !tag || !asset || extra.length ||
          (row.tag && tag !== row.tag) || (row.asset ? asset !== row.asset : !row.pattern.test(asset))) {
        throw new Error(`Unexpected asset URL for ${dest}: ${url}`);
      }
    }
    expected.delete(key);
  }
  if (expected.size) throw new Error(`Missing manifest rows:\n${[...expected.keys()].join("\n")}`);
}

if (options["update-pins"]) {
  let updated = source;
  for (const { repo, pin } of Object.values(toolReleases)) {
    const latest = (await release(repo)).tag_name.replace(/^v/, "");
    const current = constant(pin);
    const parts = (version) => version.split(".").map(Number);
    if (!/^\d+\.\d+\.\d+$/.test(latest)) throw new Error(`Unexpected release version for ${repo}: ${latest}`);
    const currentParts = parts(current);
    const firstChange = parts(latest).findIndex((number, index) => number !== currentParts[index]);
    if (firstChange >= 0 && parts(latest)[firstChange] > currentParts[firstChange]) {
      updated = updated.replace(`${pin}: &str = "${current}"`, `${pin}: &str = "${latest}"`);
      console.log(`${repo}: ${current} → ${latest}`);
    }
  }
  if (updated !== source) await writeFile(depsPath, updated);
} else {
  const rows = [...toolRows(), ...fontRows()];
  if (options.check) {
    checkManifest(await readFile(options.check, "utf8"), rows);
    console.log(`Verified ${options.check} against deps.rs (${rows.length} assets)`);
  } else {
    const tsv = [...comments, ...await Promise.all(rows.map(renderRow))].join("\n") + "\n";
    checkManifest(tsv, rows);
    await mkdir(dirname(options.out), { recursive: true });
    await writeFile(options.out, tsv);
    console.log(`Wrote ${options.out} (${rows.length} assets)`);
  }
}
