#!/usr/bin/env node
// Regenerates the theme listing block inside README.md from `data/themes/*.yml`.

import fs from 'node:fs';
import path from 'node:path';
import { readAllEntries } from './lib/entries.mjs';

const rootDir = process.cwd();
const entries = readAllEntries(rootDir);

const lines =
  entries.length === 0
    ? ['_No community themes listed yet — open the first PR to `data/themes/<id>.yml`!_']
    : entries.map((entry) => {
        const desc = entry.description?.en || entry.description?.zh || `Community theme from ${entry.repo}.`;
        const url =
          entry.subpath && entry.subpath !== '.'
            ? `https://github.com/${entry.repo}/tree/HEAD/${entry.subpath}`
            : `https://github.com/${entry.repo}`;
        return `- [\`${entry.id}\`](${url}) (${entry.repo}) — ${desc}`;
      });

const readmePath = path.join(rootDir, 'README.md');
const current = fs.readFileSync(readmePath, 'utf8');
const begin = '<!-- BEGIN THEMES -->';
const end = '<!-- END THEMES -->';
const startIdx = current.indexOf(begin);
const endIdx = current.indexOf(end);
if (startIdx < 0 || endIdx < 0 || endIdx <= startIdx) {
  throw new Error('README.md is missing <!-- BEGIN THEMES --> / <!-- END THEMES --> markers');
}

const next = `${current.slice(0, startIdx + begin.length)}\n${lines.join('\n')}\n${current.slice(endIdx)}`;
if (next !== current) {
  fs.writeFileSync(readmePath, next);
  console.log(`✓ updated README.md (${entries.length} theme(s))`);
} else {
  console.log(`· README.md already up to date (${entries.length} theme(s))`);
}
