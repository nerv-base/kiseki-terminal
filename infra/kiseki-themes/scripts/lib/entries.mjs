// Core YAML entry parsing, flat short-ID validation, denylist filtering,
// and trending score computation for nerv-base/kiseki-themes (RFC-0006).

import fs from 'node:fs';
import path from 'node:path';

export const ID_PATTERN = /^[a-z0-9][a-z0-9-]{2,47}$/;
export const REPO_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,99}\/[A-Za-z0-9][A-Za-z0-9._-]{0,99}$/;

export const RESERVED_IDS = new Set([
  'nerv-eva',
  'kyoto-sakura',
  'shinkai-rain',
  'starlight-8bit',
  'default',
  'system',
  'custom',
  'official',
  'kiseki',
]);

export const RESERVED_PREFIXES = ['kiseki-', 'official-'];

export function isReservedId(id) {
  if (RESERVED_IDS.has(id)) return true;
  return RESERVED_PREFIXES.some((prefix) => id.startsWith(prefix));
}

export function normalizeSubpath(raw) {
  if (!raw || raw.trim() === '' || raw.trim() === '.') return '.';
  const cleaned = raw.trim().replace(/^\/+|\/+$/g, '');
  const parts = cleaned.split('/');
  for (const part of parts) {
    if (!part || part === '.' || part === '..' || part.includes('\\')) {
      throw new Error(`invalid subpath "${raw}": must be a clean relative path without '..'`);
    }
  }
  return cleaned;
}

/**
 * Minimal strict YAML parser tailored for `data/themes/<id>.yml` entries.
 * Supports top-level scalar keys and the nested `description:` mapping.
 */
export function parseThemeYaml(raw, filename = '<input>') {
  const result = {};
  let currentMapKey = null;

  for (const [idx, rawLine] of raw.split(/\r?\n/).entries()) {
    const lineNo = idx + 1;
    const trimmed = rawLine.trim();
    if (!trimmed) continue;
    if (trimmed.startsWith('#')) {
      throw new Error(`${filename}:${lineNo}: YAML comments are disallowed in entry files`);
    }

    const indent = rawLine.length - rawLine.trimStart().length;
    if (indent > 0) {
      if (!currentMapKey) {
        throw new Error(`${filename}:${lineNo}: unexpected indented line`);
      }
      const sep = trimmed.indexOf(':');
      if (sep < 0) {
        throw new Error(`${filename}:${lineNo}: expected key: value inside ${currentMapKey}`);
      }
      const key = trimmed.slice(0, sep).trim();
      const val = unquoteYamlScalar(trimmed.slice(sep + 1).trim(), filename, lineNo);
      result[currentMapKey][key] = val;
      continue;
    }

    currentMapKey = null;
    const sep = trimmed.indexOf(':');
    if (sep < 0) {
      throw new Error(`${filename}:${lineNo}: expected key: value`);
    }
    const key = trimmed.slice(0, sep).trim();
    const rest = trimmed.slice(sep + 1).trim();
    if (!rest) {
      result[key] = {};
      currentMapKey = key;
    } else {
      result[key] = unquoteYamlScalar(rest, filename, lineNo);
    }
  }

  return result;
}

function unquoteYamlScalar(raw, filename, lineNo) {
  if ((raw.startsWith('"') && raw.endsWith('"')) || (raw.startsWith("'") && raw.endsWith("'"))) {
    const inner = raw.slice(1, -1);
    if (raw.startsWith('"')) {
      return inner.replace(/\\"/g, '"').replace(/\\\\/g, '\\');
    }
    return inner.replace(/''/g, "'");
  }
  if (raw.includes(': ')) {
    throw new Error(`${filename}:${lineNo}: scalar containing ": " must be quoted`);
  }
  return raw;
}

const ALLOWED_TOP_KEYS = new Set(['id', 'repo', 'subpath', 'track', 'description']);
const ALLOWED_DESC_KEYS = new Set(['en', 'zh']);

export function validateThemeEntry(entry, expectedFilenameSlug, denylist = { ids: [], repos: [] }) {
  const errors = [];
  for (const key of Object.keys(entry)) {
    if (!ALLOWED_TOP_KEYS.has(key)) {
      errors.push(`unknown field "${key}"`);
    }
  }

  const id = typeof entry.id === 'string' ? entry.id.trim() : '';
  if (!ID_PATTERN.test(id)) {
    errors.push(`invalid id "${id}": must match ${ID_PATTERN}`);
  }
  if (isReservedId(id)) {
    errors.push(`id "${id}" is reserved`);
  }
  if (expectedFilenameSlug && id !== expectedFilenameSlug) {
    errors.push(`id "${id}" must match filename "${expectedFilenameSlug}.yml"`);
  }

  const repo = typeof entry.repo === 'string' ? entry.repo.trim() : '';
  if (!REPO_PATTERN.test(repo)) {
    errors.push(`invalid repo "${repo}": expected GitHub "owner/repo"`);
  }

  let subpath = '.';
  try {
    subpath = normalizeSubpath(entry.subpath ?? '.');
  } catch (err) {
    errors.push(err.message);
  }

  const track = (entry.track ?? 'releases').toString().trim();
  if (track !== 'releases' && track !== 'branch') {
    errors.push(`invalid track "${track}": expected "releases" or "branch"`);
  }

  const description = {};
  if (entry.description !== undefined) {
    if (typeof entry.description !== 'object' || entry.description === null) {
      errors.push('description must be a mapping with en/zh keys');
    } else {
      for (const [k, v] of Object.entries(entry.description)) {
        if (!ALLOWED_DESC_KEYS.has(k)) {
          errors.push(`unknown description key "${k}"`);
        } else if (typeof v !== 'string' || v.includes('\n') || v.length > 240) {
          errors.push(`description.${k} must be a single line (<= 240 chars)`);
        } else {
          description[k] = v.trim();
        }
      }
    }
  }

  const deniedIds = new Set(denylist.ids ?? []);
  const deniedRepos = new Set((denylist.repos ?? []).map((r) => r.toLowerCase()));
  if (deniedIds.has(id) || deniedRepos.has(repo.toLowerCase())) {
    errors.push(`entry "${id}" (${repo}) is on denylist.json`);
  }

  return {
    ok: errors.length === 0,
    errors,
    normalized: { id, repo, subpath, track, description },
  };
}

export function loadDenylist(rootDir) {
  const p = path.join(rootDir, 'data', 'denylist.json');
  if (!fs.existsSync(p)) return { schemaVersion: 1, ids: [], repos: [] };
  return JSON.parse(fs.readFileSync(p, 'utf8'));
}

export function readAllEntries(rootDir) {
  const dir = path.join(rootDir, 'data', 'themes');
  if (!fs.existsSync(dir)) return [];
  const denylist = loadDenylist(rootDir);
  const files = fs
    .readdirSync(dir)
    .filter((f) => f.endsWith('.yml'))
    .sort();
  const entries = [];
  const seenRepos = new Map();

  for (const file of files) {
    const slug = file.slice(0, -4);
    const fullPath = path.join(dir, file);
    const parsed = parseThemeYaml(fs.readFileSync(fullPath, 'utf8'), file);
    const check = validateThemeEntry(parsed, slug, denylist);
    if (!check.ok) {
      throw new Error(`${file}: ${check.errors.join('; ')}`);
    }
    const repoKey = `${check.normalized.repo.toLowerCase()}#${check.normalized.subpath}`;
    if (seenRepos.has(repoKey)) {
      throw new Error(
        `${file}: duplicate repo+subpath already registered under "${seenRepos.get(repoKey)}"`,
      );
    }
    seenRepos.set(repoKey, check.normalized.id);
    entries.push(check.normalized);
  }
  return entries;
}

/**
 * Multi-dimensional trending score matching `crates/kiseki-gallery/src/model.rs`.
 */
export function computeTrendingScore({ downloads30d = 0, stars7d = 0, stars = 0, recency = 0.5 }) {
  const clampedRecency = Math.max(0, Math.min(1, recency));
  const raw =
    0.5 * Math.log(1 + Math.max(0, downloads30d)) +
    0.3 * Math.log(1 + Math.max(0, stars7d) * 4) +
    0.15 * Math.log(1 + Math.max(0, stars)) +
    0.05 * clampedRecency;
  return Number(raw.toFixed(4));
}

export function recencyFactor(updatedAtIso, nowMs = Date.now()) {
  if (!updatedAtIso) return 0;
  const ts = Date.parse(updatedAtIso);
  if (Number.isNaN(ts)) return 0;
  const ageDays = Math.max(0, (nowMs - ts) / 86400000);
  return Math.exp(-ageDays / 90);
}
