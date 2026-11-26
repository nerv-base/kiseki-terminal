#!/usr/bin/env node
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const siteRoot = resolve(__dirname, '..');
const repoRoot = resolve(siteRoot, '..');

const THEME_IDS = ['starlight-8bit', 'kyoto-sakura', 'shinkai-rain', 'nerv-eva'];
const ANSI_RE = /\x1b\[([0-9;]*)m/g;

function escapeHtml(str) {
  return str
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

/** Convert 24-bit TrueColor ANSI block-art into styled HTML spans. */
function ansiToHtml(ansiText) {
  return ansiText
    .split(/\r?\n/u)
    .map((rawLine) => {
      let fg = null;
      let bg = null;
      let buf = '';
      let pos = 0;
      const parts = [];

      const flush = () => {
        if (!buf) return;
        const text = escapeHtml(buf);
        buf = '';
        const styles = [];
        if (fg) styles.push(`color:${fg}`);
        if (bg) styles.push(`background:${bg}`);
        parts.push(styles.length ? `<span style="${styles.join(';')}">${text}</span>` : text);
      };

      for (const match of rawLine.matchAll(ANSI_RE)) {
        const idx = match.index ?? 0;
        if (idx > pos) {
          buf += rawLine.slice(pos, idx);
        }
        flush();
        pos = idx + match[0].length;

        const codes = match[1] ? match[1].split(';').filter(Boolean).map(Number) : [0];
        for (let i = 0; i < codes.length; i++) {
          const code = codes[i];
          if (code === 0) {
            fg = null;
            bg = null;
          } else if (code === 38 && i + 4 < codes.length && codes[i + 1] === 2) {
            fg = `rgb(${codes[i + 2]},${codes[i + 3]},${codes[i + 4]})`;
            i += 4;
          } else if (code === 48 && i + 4 < codes.length && codes[i + 1] === 2) {
            bg = `rgb(${codes[i + 2]},${codes[i + 3]},${codes[i + 4]})`;
            i += 4;
          }
        }
      }

      if (pos < rawLine.length) {
        buf += rawLine.slice(pos);
      }
      flush();
      return parts.join('');
    })
    .join('\n');
}

function main() {
  const themes = {};

  for (const id of THEME_IDS) {
    const themeDir = resolve(repoRoot, 'themes', id);
    const manifest = JSON.parse(readFileSync(resolve(themeDir, 'manifest.json'), 'utf8'));
    const tokens = JSON.parse(readFileSync(resolve(themeDir, 'tokens.json'), 'utf8'));
    const shaderGlsl = readFileSync(resolve(themeDir, 'shaders', 'live.glsl'), 'utf8');
    const logoTxt = readFileSync(resolve(themeDir, 'assets', 'logo.txt'), 'utf8');

    const wallPath = resolve(siteRoot, 'public', 'wallpapers', `${id}.webp`);
    const thumbPath = resolve(siteRoot, 'public', 'wallpapers', `${id}-thumb.webp`);
    if (!existsSync(wallPath)) {
      throw new Error(`Missing wallpaper WebP: ${wallPath}`);
    }
    if (!existsSync(thumbPath)) {
      throw new Error(`Missing thumbnail WebP: ${thumbPath}`);
    }

    themes[id] = {
      manifest,
      tokens,
      shaderGlsl,
      logoHtml: ansiToHtml(logoTxt),
      wallpaperUrl: `/wallpapers/${id}.webp`,
      thumbUrl: `/wallpapers/${id}-thumb.webp`,
    };
  }

  const outPath = resolve(siteRoot, 'src', 'generated', 'themes.json');
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, `${JSON.stringify({ themes }, null, 2)}\n`, 'utf8');

  for (const scriptName of ['install.sh', 'install.ps1']) {
    const srcScript = resolve(repoRoot, 'scripts', scriptName);
    const dstScript = resolve(siteRoot, 'public', scriptName);
    copyFileSync(srcScript, dstScript);
  }

  console.log(`[sync:themes] Generated ${outPath} (${THEME_IDS.length} themes) & synced install scripts`);
}

main();
