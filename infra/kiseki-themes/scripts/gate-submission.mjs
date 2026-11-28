#!/usr/bin/env node
// Trusted-context PR submission gate (`pr-gate.yml`, triggered via `workflow_run`).
// SECURITY: Runs strictly from the base repo's default branch. Reads changed
// YAML files extracted into `--entries-dir` purely as data, downloads the
// target GitHub repo's archive, and runs `kiseki theme validate --json`.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { loadDenylist, parseThemeYaml, validateThemeEntry } from './lib/entries.mjs';

const args = process.argv.slice(2);
const entriesDirIdx = args.indexOf('--entries-dir');
if (entriesDirIdx < 0 || !args[entriesDirIdx + 1]) {
  console.error('usage: node scripts/gate-submission.mjs --entries-dir <dir>');
  process.exit(2);
}

const entriesDir = args[entriesDirIdx + 1];
const kisekiBin = process.env.KISEKI_BIN || 'kiseki';
const ghToken = process.env.GITHUB_TOKEN || '';
const denylist = loadDenylist(process.cwd());

async function ghJson(url) {
  const headers = {
    'User-Agent': 'kiseki-themes-gate',
    Accept: 'application/vnd.github+json',
  };
  if (ghToken) headers.Authorization = `Bearer ${ghToken}`;
  const res = await fetch(url, { headers });
  if (!res.ok) {
    throw new Error(`GitHub API ${res.status} for ${url}`);
  }
  return res.json();
}

async function resolveTargetCommit(repo, track) {
  const repoInfo = await ghJson(`https://api.github.com/repos/${repo}`);
  if (repoInfo.private) throw new Error(`${repo} is private`);
  if (repoInfo.archived) throw new Error(`${repo} is archived`);

  if (track === 'releases') {
    try {
      const rel = await ghJson(`https://api.github.com/repos/${repo}/releases/latest`);
      if (rel?.tag_name) {
        const ref = await ghJson(
          `https://api.github.com/repos/${repo}/commits/${encodeURIComponent(rel.tag_name)}`,
        );
        if (ref?.sha) return { commit: ref.sha, ref: rel.tag_name };
      }
    } catch {
      // No GitHub Release yet; fall back cleanly to default branch head.
    }
  }

  const branch = repoInfo.default_branch || 'main';
  const head = await ghJson(
    `https://api.github.com/repos/${repo}/commits/${encodeURIComponent(branch)}`,
  );
  if (!head?.sha) throw new Error(`could not resolve commit for ${repo}@${branch}`);
  return { commit: head.sha, ref: branch };
}

async function main() {
  const files = fs
    .readdirSync(entriesDir)
    .filter((f) => f.endsWith('.yml'))
    .sort();
  if (files.length === 0) {
    console.log('No theme YAML entries changed in this PR.');
    return;
  }

  const warnings = [];
  for (const file of files) {
    const slug = file.slice(0, -4);
    const parsed = parseThemeYaml(fs.readFileSync(path.join(entriesDir, file), 'utf8'), file);
    const check = validateThemeEntry(parsed, slug, denylist);
    if (!check.ok) {
      throw new Error(`${file}: ${check.errors.join('; ')}`);
    }

    const { id, repo, subpath, track } = check.normalized;
    const { commit, ref } = await resolveTargetCommit(repo, track);
    console.log(`→ gating ${id} (${repo}@${ref} -> ${commit.slice(0, 8)})`);

    const archiveUrl = `https://codeload.github.com/${repo}/tar.gz/${commit}`;
    const res = await fetch(archiveUrl, { headers: { 'User-Agent': 'kiseki-themes-gate' } });
    if (!res.ok) throw new Error(`failed to fetch archive ${archiveUrl} (HTTP ${res.status})`);
    const tarBuf = Buffer.from(await res.arrayBuffer());
    if (tarBuf.length > 30 * 1024 * 1024) {
      throw new Error(`${id}: archive exceeds 30 MiB hard limit`);
    }

    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `kiseki-gate-${id}-`));
    try {
      const tarPath = path.join(tmp, 'pack.tar.gz');
      const unpackDir = path.join(tmp, 'unpacked');
      fs.writeFileSync(tarPath, tarBuf);
      fs.mkdirSync(unpackDir);
      execFileSync('tar', ['-xzf', tarPath, '-C', unpackDir]);

      const roots = fs.readdirSync(unpackDir);
      if (roots.length !== 1) {
        throw new Error(`${id}: expected single top-level archive folder, found ${roots.length}`);
      }
      const themeDir =
        subpath === '.' ? path.join(unpackDir, roots[0]) : path.join(unpackDir, roots[0], subpath);
      if (!fs.existsSync(path.join(themeDir, 'manifest.json'))) {
        throw new Error(`${id}: manifest.json not found at subpath "${subpath}"`);
      }

      const manifest = JSON.parse(fs.readFileSync(path.join(themeDir, 'manifest.json'), 'utf8'));
      if (manifest.id !== id) {
        throw new Error(`${id}: manifest.json#id ("${manifest.id}") does not match entry id "${id}"`);
      }

      const out = execFileSync(kisekiBin, ['theme', 'validate', themeDir, '--json'], {
        encoding: 'utf8',
      });
      const report = JSON.parse(out);
      if (!report.valid) {
        throw new Error(`${id}: kiseki theme validate failed`);
      }

      const tokens = JSON.parse(fs.readFileSync(path.join(themeDir, 'tokens.json'), 'utf8'));
      const hasVisuals = Boolean(tokens?.atmosphere?.wallpaper || tokens?.atmosphere?.shader);
      const hasPreview = fs.existsSync(path.join(themeDir, 'previews', 'preview.png'));
      if (hasVisuals && !hasPreview) {
        warnings.push(
          `Tip for \`${id}\`: add \`previews/preview.png\` (a terminal screenshot) so the Gallery card showcases your wallpaper/shader!`,
        );
      }
      console.log(`✓ ${id} passed full sandbox + naga + WCAG validation (${report.files} files)`);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  }

  for (const w of warnings) {
    console.log(`::warning::${w}`);
  }
}

main().catch((err) => {
  console.error(`✗ pr-gate failed: ${err.message}`);
  process.exit(1);
});
