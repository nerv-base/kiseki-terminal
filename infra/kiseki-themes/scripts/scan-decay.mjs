#!/usr/bin/env node
// Weekly decay sweep (`decay-scan.yml`): reports deleted, private, archived,
// or invalid repositories into a single GitHub Tracking Issue for human review.
// Never auto-deletes entries silently.

import { readAllEntries } from './lib/entries.mjs';

const rootDir = process.cwd();
const ghToken = process.env.GITHUB_TOKEN || '';
const registryRepo = process.env.GITHUB_REPOSITORY || '';

async function gh(url, init = {}) {
  const headers = {
    'User-Agent': 'kiseki-themes-decay-scan',
    Accept: 'application/vnd.github+json',
    ...init.headers,
  };
  if (ghToken) headers.Authorization = `Bearer ${ghToken}`;
  return fetch(url, { ...init, headers });
}

async function main() {
  const entries = readAllEntries(rootDir);
  const findings = [];

  for (const entry of entries) {
    const res = await gh(`https://api.github.com/repos/${entry.repo}`);
    if (res.status === 404) {
      findings.push(`- \`${entry.id}\` (${entry.repo}): repository not found or private (404)`);
      continue;
    }
    if (!res.ok) continue;
    const data = await res.json();
    if (data.archived) {
      findings.push(`- \`${entry.id}\` (${entry.repo}): repository is archived`);
    }
  }

  console.log(`Decay scan inspected ${entries.length} entries, found ${findings.length} issue(s).`);
  if (findings.length === 0 || !ghToken || !registryRepo) return;

  const title = 'Weekly Decay Scan — Unreachable / Archived Themes';
  const body = [
    `Automated weekly check (${new Date().toISOString().slice(0, 10)}) flagged ${findings.length} theme(s):`,
    '',
    ...findings,
    '',
    'Review and remove dead entries from `data/themes/` or add to `data/denylist.json`.',
  ].join('\n');

  const listRes = await gh(
    `https://api.github.com/repos/${registryRepo}/issues?state=open&per_page=50`,
  );
  const openIssues = listRes.ok ? await listRes.json() : [];
  const existing = openIssues.find((i) => i.title === title);

  if (existing) {
    await gh(`https://api.github.com/repos/${registryRepo}/issues/${existing.number}`, {
      method: 'PATCH',
      body: JSON.stringify({ body }),
    });
    console.log(`✓ updated tracking issue #${existing.number}`);
  } else {
    await gh(`https://api.github.com/repos/${registryRepo}/issues`, {
      method: 'POST',
      body: JSON.stringify({ title, body }),
    });
    console.log('✓ created decay tracking issue');
  }
}

main().catch((err) => {
  console.error(`✗ decay-scan failed: ${err.message}`);
  process.exit(1);
});
