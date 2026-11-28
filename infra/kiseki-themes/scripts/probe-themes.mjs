#!/usr/bin/env node
// Probes registered repositories via GitHub GraphQL (batched), validates changed
// commits with `kiseki theme validate --json`, converts `previews/preview.png`
// to <=960px WebP thumbnails, and writes sharded probe caches.
//
// Fault isolation (RFC-0006 §2.4): if a newly released commit fails validation,
// the previously validated commit remains live in `data/probe-state.json` and
// the failure is recorded only in `dist/sync-status.json`.

import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readAllEntries } from './lib/entries.mjs';

const rootDir = process.cwd();
const ghToken = process.env.GITHUB_TOKEN || '';
const kisekiBin = process.env.KISEKI_BIN || 'kiseki';
const targetRepo = (process.env.PROBE_TARGET_REPO || '').trim();
const statsUrl = process.env.GALLERY_STATS_URL || 'https://dl.kisekiterm.dev/gallery/stats.json';
const cdnOrigin = (process.env.GALLERY_CDN_ORIGIN || 'https://dl.kisekiterm.dev').replace(/\/+$/, '');
const outAssetsDir = path.join(rootDir, 'dist', 'r2-assets');

function readJson(p, fallback) {
  return fs.existsSync(p) ? JSON.parse(fs.readFileSync(p, 'utf8')) : fallback;
}

function writeJson(p, obj) {
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, JSON.stringify(obj, null, 2) + '\n');
}

async function ghGraphql(query) {
  const headers = {
    'User-Agent': 'kiseki-themes-probe',
    'Content-Type': 'application/json',
  };
  if (ghToken) headers.Authorization = `Bearer ${ghToken}`;
  const res = await fetch('https://api.github.com/graphql', {
    method: 'POST',
    headers,
    body: JSON.stringify({ query }),
  });
  if (!res.ok) throw new Error(`GitHub GraphQL HTTP ${res.status}`);
  const payload = await res.json();
  if (payload.errors?.length) {
    console.warn('GraphQL partial warnings:', payload.errors.map((e) => e.message).join('; '));
  }
  return payload.data || {};
}

async function probeGitHubBatch(entries) {
  const meta = new Map();
  const batchSize = 50;
  for (let i = 0; i < entries.length; i += batchSize) {
    const chunk = entries.slice(i, i + batchSize);
    const blocks = chunk
      .map((entry, idx) => {
        const [owner, name] = entry.repo.split('/');
        return `r${idx}: repository(owner: ${JSON.stringify(owner)}, name: ${JSON.stringify(name)}) {
          isArchived
          stargazerCount
          pushedAt
          defaultBranchRef {
            name
            target { ... on Commit { oid committedDate } }
          }
          latestRelease {
            tagName
            publishedAt
            description
            tagCommit { oid committedDate }
          }
        }`;
      })
      .join('\n');
    const data = await ghGraphql(`query { ${blocks} }`);
    chunk.forEach((entry, idx) => {
      const node = data[`r${idx}`];
      if (node) meta.set(entry.id, node);
    });
  }
  return meta;
}

function tryConvertWebp(srcPng, destWebp) {
  fs.mkdirSync(path.dirname(destWebp), { recursive: true });
  try {
    // Prefer cwebp when available on Ubuntu GitHub runners (`sudo apt-get install webp`).
    execFileSync('cwebp', ['-q', '80', '-resize', '960', '0', srcPng, '-o', destWebp], {
      stdio: 'ignore',
    });
    const stat = fs.statSync(destWebp);
    if (stat.size <= 150 * 1024) return true;
    // Retry at tighter quality if screenshot is unusually dense.
    execFileSync('cwebp', ['-q', '62', '-resize', '960', '0', srcPng, '-o', destWebp], {
      stdio: 'ignore',
    });
    return true;
  } catch {
    return false;
  }
}

function extractPalettePreview(tokens) {
  const p = tokens?.palette ?? {};
  const s = tokens?.syntax ?? {};
  return {
    base: p?.surfaces?.base || '#131722',
    surface0: p?.surfaces?.surface0 || '#1D2333',
    textPrimary: p?.text?.primary || '#E9EEF8',
    accentPrimary: p?.accents?.primary || '#74C7EC',
    accentSecondary: p?.accents?.secondary || '#F5A9B8',
    keyword: s.keyword || p?.accents?.secondary || '#CBA6F7',
    string: s.string || p?.ansi?.green || '#A6E3A1',
    function: s.function || p?.accents?.primary || '#89B4FA',
  };
}

function computeWcagGrade(contrastChecks = []) {
  if (!Array.isArray(contrastChecks) || contrastChecks.length === 0) return 'AA';
  const primary = contrastChecks.find((c) => c.name === 'text.primary/base');
  if (primary && primary.ratio >= 7.0) return 'AAA';
  return 'AA';
}

async function fetchDownloadStats(existing) {
  try {
    const res = await fetch(statsUrl, { headers: { 'User-Agent': 'kiseki-themes-probe' } });
    if (!res.ok) return existing;
    const data = await res.json();
    if (data && typeof data === 'object') {
      return { ...existing, ...(data.themes || data) };
    }
  } catch {
    // Non-fatal: keep cached download counts if Worker endpoint is unreachable.
  }
  return existing;
}

async function main() {
  const allEntries = readAllEntries(rootDir);
  const probeState = readJson(path.join(rootDir, 'data', 'probe-state.json'), {});
  const starsCache = readJson(path.join(rootDir, 'data', 'stars.json'), {});
  let downloadsCache = readJson(path.join(rootDir, 'data', 'downloads.json'), {});
  const updatesCache = readJson(path.join(rootDir, 'data', 'updates.json'), {
    schemaVersion: 1,
    themes: {},
  });
  const syncStatus = { generatedAt: new Date().toISOString(), entries: {} };

  const selected = targetRepo
    ? allEntries.filter(
        (e) =>
          e.id === targetRepo ||
          e.repo.toLowerCase() === targetRepo.toLowerCase(),
      )
    : allEntries;

  if (!targetRepo) {
    downloadsCache = await fetchDownloadStats(downloadsCache);
    writeJson(path.join(rootDir, 'data', 'downloads.json'), downloadsCache);
  }

  if (selected.length === 0) {
    console.log('No entries to probe.');
    writeJson(path.join(rootDir, 'dist', 'sync-status.json'), syncStatus);
    return;
  }

  const ghMeta = await probeGitHubBatch(selected);

  for (const entry of selected) {
    const info = ghMeta.get(entry.id);
    if (!info || info.isArchived) {
      syncStatus.entries[entry.id] = {
        status: 'unreachable-or-archived',
        keptPrevious: Boolean(probeState[entry.id]),
      };
      continue;
    }

    starsCache[entry.id] = {
      stars: info.stargazerCount ?? 0,
      pushedAt: info.pushedAt ?? '',
    };

    const release = info.latestRelease;
    const useRelease = entry.track === 'releases' && release?.tagCommit?.oid;
    const commit = useRelease
      ? release.tagCommit.oid
      : info.defaultBranchRef?.target?.oid || '';
    const updatedAt = useRelease
      ? release.publishedAt || release.tagCommit.committedDate || info.pushedAt
      : info.defaultBranchRef?.target?.committedDate || info.pushedAt;

    if (!commit) {
      syncStatus.entries[entry.id] = { status: 'no-commit', keptPrevious: Boolean(probeState[entry.id]) };
      continue;
    }

    if (useRelease && release.description) {
      updatesCache.themes[entry.id] = {
        latestTag: release.tagName,
        releaseNotes: release.description.slice(0, 4000),
        recentCommits: [],
      };
    }

    const prev = probeState[entry.id];
    if (prev && prev.commit === commit && !process.env.PROBE_ALL) {
      syncStatus.entries[entry.id] = { status: 'unchanged', commit };
      continue;
    }

    // Download immutable tarball and run real `kiseki theme validate --json`.
    const archiveUrl = `https://codeload.github.com/${entry.repo}/tar.gz/${commit}`;
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `kiseki-probe-${entry.id}-`));
    try {
      const res = await fetch(archiveUrl, { headers: { 'User-Agent': 'kiseki-themes-probe' } });
      if (!res.ok) throw new Error(`HTTP ${res.status} fetching archive`);
      const tarBuf = Buffer.from(await res.arrayBuffer());
      if (tarBuf.length > 30 * 1024 * 1024) throw new Error('archive exceeds 30 MiB');
      const sha256 = crypto.createHash('sha256').update(tarBuf).digest('hex');

      const tarPath = path.join(tmp, 'pack.tar.gz');
      const unpackDir = path.join(tmp, 'unpacked');
      fs.writeFileSync(tarPath, tarBuf);
      fs.mkdirSync(unpackDir);
      execFileSync('tar', ['-xzf', tarPath, '-C', unpackDir]);
      const topDirs = fs.readdirSync(unpackDir);
      if (topDirs.length !== 1) throw new Error('expected single archive wrapper directory');
      const stripPrefix = topDirs[0];
      const themeDir =
        entry.subpath === '.'
          ? path.join(unpackDir, stripPrefix)
          : path.join(unpackDir, stripPrefix, entry.subpath);

      const validationRaw = execFileSync(kisekiBin, ['theme', 'validate', themeDir, '--json'], {
        encoding: 'utf8',
      });
      const validation = JSON.parse(validationRaw);
      if (!validation.valid) {
        throw new Error('kiseki theme validate returned invalid');
      }

      const manifest = JSON.parse(fs.readFileSync(path.join(themeDir, 'manifest.json'), 'utf8'));
      if (manifest.id !== entry.id) {
        throw new Error(`manifest.id "${manifest.id}" != "${entry.id}"`);
      }
      const tokens = JSON.parse(fs.readFileSync(path.join(themeDir, 'tokens.json'), 'utf8'));
      const hasWallpaper = Boolean(
        tokens?.atmosphere?.wallpaper ||
          ['wallpaper.png', 'wallpaper.jpg', 'wallpaper.jpeg', 'wallpaper.webp'].some((f) =>
            fs.existsSync(path.join(themeDir, 'assets', f)),
          ),
      );
      const hasShader = Boolean(tokens?.atmosphere?.shader);

      // Generate WebP screenshot thumbnail (<= 960px, <= 150 KiB) for R2 acceleration.
      let thumbUrl = null;
      const previewPng = path.join(themeDir, 'previews', 'preview.png');
      if (fs.existsSync(previewPng)) {
        const thumbName = `${entry.id}-${commit.slice(0, 8)}.webp`;
        const destWebp = path.join(outAssetsDir, 'thumbs', thumbName);
        if (tryConvertWebp(previewPng, destWebp)) {
          thumbUrl = `${cdnOrigin}/gallery/thumbs/${thumbName}`;
        }
      }

      // Mirror wallpaper-free themes directly into R2 archives for fast direct downloads.
      let mirrorArchive = false;
      if (!hasWallpaper && manifest?.provenance?.wallpaperOrigin === 'none') {
        const destArchive = path.join(outAssetsDir, 'archives', `${entry.id}-${commit}.tar.gz`);
        fs.mkdirSync(path.dirname(destArchive), { recursive: true });
        fs.writeFileSync(destArchive, tarBuf);
        mirrorArchive = true;
      }

      probeState[entry.id] = {
        id: entry.id,
        name: manifest.name,
        subtitle: entry.description?.en || manifest.subtitle || '',
        version: manifest.version || '0.1.0',
        appearance: manifest.appearance || 'dark',
        tags: Array.isArray(manifest.tags) ? manifest.tags : [],
        authors: Array.isArray(manifest.authors) ? manifest.authors : [],
        license: manifest.license || 'MIT',
        repo: entry.repo,
        subpath: entry.subpath,
        commit,
        createdAt: prev?.createdAt || updatedAt || new Date().toISOString(),
        updatedAt: updatedAt || new Date().toISOString(),
        capabilities: {
          hasWallpaper,
          hasShader,
          shaderProfiles: hasShader ? ['eco', 'balanced', 'ultra'] : [],
          wcagGrade: computeWcagGrade(validation.contrast),
        },
        download: {
          trackUrl: `${cdnOrigin}/gallery/dl/${entry.id}/${commit}`,
          url: archiveUrl,
          sha256,
          bytes: tarBuf.length,
          stripPrefix,
          r2Mirrored: mirrorArchive,
        },
        preview: {
          thumbUrl,
          palette: extractPalettePreview(tokens),
        },
      };
      syncStatus.entries[entry.id] = { status: 'ok', commit, hasPreview: Boolean(thumbUrl) };
      console.log(`✓ probed ${entry.id}@${commit.slice(0, 8)} (v${manifest.version})`);
    } catch (err) {
      // Retain previous known-good entry untouched.
      syncStatus.entries[entry.id] = {
        status: 'failed',
        commit,
        error: err.message,
        keptPrevious: Boolean(prev),
      };
      console.warn(`! probe failed for ${entry.id}@${commit.slice(0, 8)}: ${err.message}`);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  }

  writeJson(path.join(rootDir, 'data', 'probe-state.json'), probeState);
  writeJson(path.join(rootDir, 'data', 'stars.json'), starsCache);
  writeJson(path.join(rootDir, 'data', 'updates.json'), updatesCache);
  writeJson(path.join(rootDir, 'dist', 'sync-status.json'), syncStatus);
}

main().catch((err) => {
  console.error(`✗ probe-themes failed: ${err.message}`);
  process.exit(1);
});
