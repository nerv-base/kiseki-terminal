import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { buildGalleryIndex } from './build-index.mjs';
import {
  computeTrendingScore,
  isReservedId,
  parseThemeYaml,
  validateThemeEntry,
} from './lib/entries.mjs';

test('parses valid theme YAML and rejects comments / unquoted colons', () => {
  const ok = parseThemeYaml(
    [
      'id: tokyo-midnight',
      'repo: alice/kiseki-tokyo',
      'subpath: .',
      'track: releases',
      'description:',
      '  en: "Deep indigo: city pop glow."',
      '  zh: 深靛蓝都市夜景配色。',
    ].join('\n'),
    'tokyo-midnight.yml',
  );
  const check = validateThemeEntry(ok, 'tokyo-midnight');
  assert.equal(check.ok, true, check.errors.join(', '));
  assert.equal(check.normalized.id, 'tokyo-midnight');

  assert.throws(() => parseThemeYaml('# comment\nid: foo\n', 'foo.yml'));
  assert.throws(() =>
    parseThemeYaml('id: foo\ndescription:\n  en: unquoted: colon\n', 'foo.yml'),
  );
});

test('enforces reserved flat IDs, filename match, and denylist', () => {
  assert.equal(isReservedId('nerv-eva'), true);
  assert.equal(isReservedId('kiseki-custom'), true);
  assert.equal(isReservedId('tokyo-midnight'), false);

  const badReserved = validateThemeEntry({ id: 'nerv-eva', repo: 'a/b' }, 'nerv-eva');
  assert.equal(badReserved.ok, false);

  const badMismatch = validateThemeEntry({ id: 'tokyo-night', repo: 'a/b' }, 'other-id');
  assert.equal(badMismatch.ok, false);

  const denied = validateThemeEntry(
    { id: 'bad-theme', repo: 'evil/pack' },
    'bad-theme',
    { ids: [], repos: ['evil/pack'] },
  );
  assert.equal(denied.ok, false);
});

test('builds deterministic index with 7d star delta and multi-dimensional trending score', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'kiseki-reg-test-'));
  try {
    fs.mkdirSync(path.join(tmp, 'data', 'themes'), { recursive: true });
    fs.mkdirSync(path.join(tmp, 'snapshots'), { recursive: true });

    fs.writeFileSync(
      path.join(tmp, 'data', 'themes', 'tokyo-midnight.yml'),
      'id: tokyo-midnight\nrepo: alice/tokyo-midnight\n',
    );
    fs.writeFileSync(
      path.join(tmp, 'data', 'stars.json'),
      JSON.stringify({ 'tokyo-midnight': { stars: 150 } }),
    );
    fs.writeFileSync(
      path.join(tmp, 'data', 'downloads.json'),
      JSON.stringify({ 'tokyo-midnight': { total: 900, last30d: 400 } }),
    );
    fs.writeFileSync(
      path.join(tmp, 'snapshots', '2026-11-20.json'),
      JSON.stringify({ 'tokyo-midnight': { stars: 120, downloads: 500 } }),
    );
    fs.writeFileSync(
      path.join(tmp, 'data', 'probe-state.json'),
      JSON.stringify({
        'tokyo-midnight': {
          id: 'tokyo-midnight',
          name: 'Tokyo Midnight',
          subtitle: 'City pop indigo',
          version: '1.2.0',
          appearance: 'dark',
          tags: ['dark', 'live-shader'],
          authors: [{ name: 'Alice' }],
          license: 'MIT',
          commit: 'abcdef1234567890',
          createdAt: '2026-11-01T00:00:00Z',
          updatedAt: '2026-11-28T00:00:00Z',
          capabilities: {
            hasWallpaper: true,
            hasShader: true,
            shaderProfiles: ['eco', 'balanced', 'ultra'],
            wcagGrade: 'AA',
          },
          download: {
            trackUrl: 'https://dl.kisekiterm.dev/gallery/dl/tokyo-midnight/abcdef1234567890',
            url: 'https://codeload.github.com/alice/tokyo-midnight/tar.gz/abcdef1234567890',
            sha256: 'deadbeef',
            bytes: 12345,
            stripPrefix: 'tokyo-midnight-abcdef12',
          },
          preview: {
            thumbUrl: 'https://dl.kisekiterm.dev/gallery/thumbs/tokyo-midnight-abcdef12.webp',
            palette: { base: '#131722' },
          },
        },
      }),
    );

    const index = buildGalleryIndex(tmp, new Date('2026-11-28T12:00:00Z'));
    assert.equal(index.themes.length, 1);
    const t = index.themes[0];
    assert.equal(t.metrics.stars, 150);
    assert.equal(t.metrics.stars7d, 30);
    assert.equal(t.metrics.downloads, 900);
    assert.equal(t.metrics.downloads30d, 400);
    assert.ok(t.metrics.trendingScore > computeTrendingScore({ downloads30d: 10, stars7d: 0, stars: 10 }));
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
