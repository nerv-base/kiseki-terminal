#!/usr/bin/env node
// Fork-safe static PR check (`pr-check.yml`, runs with zero privileged tokens).
// Validates every `data/themes/*.yml` file, flat short-ID rules, denylist,
// and ensures generated READMEs were not manually edited in a submission PR.

import { execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { readAllEntries } from './lib/entries.mjs';

const rootDir = process.cwd();
const baseRef = process.env.PR_BASE_REF || '';

try {
  const entries = readAllEntries(rootDir);
  console.log(`✓ validated ${entries.length} theme entry file(s) in data/themes/`);

  if (baseRef) {
    const changedRaw = execSync(`git diff --name-only origin/${baseRef}...HEAD`, {
      encoding: 'utf8',
    }).trim();
    const changed = changedRaw ? changedRaw.split(/\r?\n/) : [];
    const addedThemes = [];

    for (const file of changed) {
      if (file === 'README.md' || file === 'README.zh.md') {
        throw new Error(
          `do not edit ${file} by hand; it is regenerated automatically after merge`,
        );
      }
      if (file.startsWith('data/themes/') && file.endsWith('.yml')) {
        // Check if this is a brand-new file vs edit.
        const status = execSync(`git diff --name-status origin/${baseRef}...HEAD -- "${file}"`, {
          encoding: 'utf8',
        }).trim();
        if (status.startsWith('A')) {
          addedThemes.push(file);
        }
      }
    }

    if (addedThemes.length > 1 && process.env.ALLOW_MULTI_SUBMIT !== '1') {
      throw new Error(
        `PR adds ${addedThemes.length} themes (${addedThemes.join(', ')}); submit 1 theme per PR to avoid namespace squatting`,
      );
    }

    if (changed.length > 0) {
      console.log(`✓ PR touches ${changed.length} file(s), ${addedThemes.length} new theme(s)`);
    }
  }
} catch (err) {
  console.error(`✗ check-submission failed: ${err.message}`);
  process.exit(1);
}
