#!/usr/bin/env node
// Compiles minified `dist/index.json`, `dist/index.json.sha256`, and `dist/updates.json`
// from `data/themes/*.yml` + sharded probe caches + 7-day metric snapshots.
// Zero network access — deterministic pure build step.

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { computeTrendingScore, readAllEntries, recencyFactor } from './lib/entries.mjs';

export function loadSnapshotBefore(rootDir, daysAgo, now = new Date()) {
  const snapDir = path.join(rootDir, 'snapshots');
  if (!fs.existsSync(snapDir)) return {};
  const target = new Date(now.getTime() - daysAgo * 86400000).toISOString().slice(0, 10);
  const files = fs
    .readdirSync(snapDir)
    .filter((f) => /^\d{4}-\d{2}-\d{2}\.json$/.test(f))
    .sort();
  if (files.length === 0) return {};
  // Pick the closest snapshot <= target, falling back to the oldest available.
  let chosen = files[0];
  for (const f of files) {
    if (f.slice(0, 10) <= target) chosen = f;
  }
  return JSON.parse(fs.readFileSync(path.join(snapDir, chosen), 'utf8'));
}

export function buildGalleryIndex(rootDir, now = new Date()) {
  const entries = readAllEntries(rootDir);
  const readJson = (rel, fallback) => {
    const p = path.join(rootDir, rel);
    return fs.existsSync(p) ? JSON.parse(fs.readFileSync(p, 'utf8')) : fallback;
  };
  const probeState = readJson('data/probe-state.json', {});
  const starsCache = readJson('data/stars.json', {});
  const downloadsCache = readJson('data/downloads.json', {});
  const snap7d = loadSnapshotBefore(rootDir, 7, now);

  const themes = [];
  for (const entry of entries) {
    const probed = probeState[entry.id];
    if (!probed) continue;
    const stars = starsCache[entry.id]?.stars ?? 0;
    const prevStars = snap7d?.[entry.id]?.stars ?? stars;
    const stars7d = Math.max(0, stars - prevStars);

    const dlRecord = downloadsCache[entry.id] ?? {};
    const downloads = typeof dlRecord === 'number' ? dlRecord : dlRecord.total ?? 0;
    const downloads30d = typeof dlRecord === 'number' ? dlRecord : dlRecord.last30d ?? downloads;

    const trendingScore = computeTrendingScore({
      downloads30d,
      stars7d,
      stars,
      recency: recencyFactor(probed.updatedAt, now.getTime()),
    });

    themes.push({
      id: probed.id,
      name: probed.name,
      subtitle: entry.description?.en || probed.subtitle || '',
      version: probed.version,
      appearance: probed.appearance,
      tags: probed.tags,
      authors: probed.authors,
      license: probed.license,
      repo: entry.repo,
      subpath: entry.subpath,
      commit: probed.commit,
      createdAt: probed.createdAt,
      updatedAt: probed.updatedAt,
      capabilities: probed.capabilities,
      metrics: {
        stars,
        stars7d,
        downloads,
        downloads30d,
        trendingScore,
      },
      download: {
        trackUrl: probed.download.trackUrl,
        url: probed.download.url,
        sha256: probed.download.sha256,
        bytes: probed.download.bytes,
        stripPrefix: probed.download.stripPrefix,
      },
      preview: probed.preview,
    });
  }

  themes.sort((a, b) => b.metrics.trendingScore - a.metrics.trendingScore || a.id.localeCompare(b.id));

  return {
    schemaVersion: 1,
    generatedAt: now.toISOString(),
    themes,
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const rootDir = process.cwd();
  const distDir = path.join(rootDir, 'dist');
  fs.mkdirSync(distDir, { recursive: true });

  const index = buildGalleryIndex(rootDir);
  // Minified JSON to keep client downloads fast and predictable.
  const minified = JSON.stringify(index) + '\n';
  const sha = crypto.createHash('sha256').update(minified).digest('hex');

  fs.writeFileSync(path.join(distDir, 'index.json'), minified);
  fs.writeFileSync(path.join(distDir, 'index.json.sha256'), `${sha}  index.json\n`);

  const updatesPath = path.join(rootDir, 'data', 'updates.json');
  const updates = fs.existsSync(updatesPath)
    ? JSON.parse(fs.readFileSync(updatesPath, 'utf8'))
    : { schemaVersion: 1, themes: {} };
  fs.writeFileSync(path.join(distDir, 'updates.json'), JSON.stringify(updates) + '\n');

  console.log(`✓ built dist/index.json (${index.themes.length} themes, sha256=${sha.slice(0, 12)})`);
}
