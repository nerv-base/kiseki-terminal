# Kiseki Gallery Cloudflare Worker

Lightweight edge worker attached to `dl.kisekiterm.dev/gallery/*` providing:

- **Anonymous download counting (`GET /gallery/dl/:id/:commit`)**: validates `(id, commit)` against R2 `gallery/index.json` (zero open-redirect), increments total & 30-day rolling anonymous counters in KV, serves R2-mirrored archives directly for wallpaper-free themes, and returns `302 Found` to the author's pinned GitHub archive for wallpaper themes.
- **Aggregated stats (`GET /gallery/stats.json`)**: consumed by `nerv-base/kiseki-themes` scheduled probe workflow to populate `metrics.downloads` and `metrics.downloads30d`.
- **CORS-enabled static serving**: serves `gallery/index.json`, `gallery/updates.json`, and `gallery/thumbs/*.webp` directly from the `kiseki-dl` R2 bucket with ETag / `304 Not Modified` support.

## Deploy

```bash
cd infra/gallery-worker
npm install
npx wrangler kv namespace create STATS
# Put the returned namespace id into wrangler.toml and uncomment the route
npx wrangler deploy
```
