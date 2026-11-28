/**
 * Kiseki Gallery Cloudflare Worker (RFC-0006 §3.2)
 *
 * Responsibilities:
 * 1. `GET /gallery/dl/:id/:commit`
 *    - Validates `(id, commit)` strictly against R2 `gallery/index.json` (zero open-redirect).
 *    - Records an anonymous download increment (total + daily bucket for 30-day rolling stats).
 *    - For wallpaper-free themes mirrored in R2 (`gallery/archives/<id>-<commit>.tar.gz`),
 *      streams directly from R2 for fast downloads.
 *    - For wallpaper themes, returns `302 Found` to the pinned immutable GitHub archive URL
 *      without proxying or hosting third-party wallpaper binaries.
 * 2. `GET /gallery/stats.json`
 *    - Returns aggregated `{ total, last30d }` counts consumed by `kiseki-themes` probe workflow.
 * 3. `GET /gallery/index.json`, `/gallery/updates.json`, `/gallery/thumbs/*`
 *    - Serves R2 objects with clean CORS headers so `kisekiterm.dev` can fetch cross-origin.
 */

interface Env {
  R2: R2Bucket;
  STATS: KVNamespace;
}

interface IndexTheme {
  id: string;
  commit: string;
  download: {
    url: string;
    sha256: string;
  };
}

interface GalleryIndex {
  themes: IndexTheme[];
}

const CORS_HEADERS: Record<string, string> = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
  'Access-Control-Max-Age': '86400',
};

const ID_RE = /^[a-z0-9][a-z0-9-]{2,47}$/;
const COMMIT_RE = /^[0-9a-f]{7,40}$/i;

function todayUtc(): string {
  return new Date().toISOString().slice(0, 10);
}

function last30DaysUtc(): string[] {
  const days: string[] = [];
  const now = Date.now();
  for (let i = 0; i < 30; i++) {
    days.push(new Date(now - i * 86400_000).toISOString().slice(0, 10));
  }
  return days;
}

async function loadIndexFromR2(r2: R2Bucket): Promise<GalleryIndex | null> {
  const obj = await r2.get('gallery/index.json');
  if (!obj) return null;
  try {
    return (await obj.json()) as GalleryIndex;
  } catch {
    return null;
  }
}

async function incrementAnonymousCount(kv: KVNamespace, id: string): Promise<void> {
  const totalKey = `total:${id}`;
  const dayKey = `day:${todayUtc()}:${id}`;
  const [curTotal, curDay] = await Promise.all([kv.get(totalKey), kv.get(dayKey)]);
  const nextTotal = (Number.parseInt(curTotal ?? '0', 10) || 0) + 1;
  const nextDay = (Number.parseInt(curDay ?? '0', 10) || 0) + 1;
  await Promise.all([
    kv.put(totalKey, String(nextTotal)),
    kv.put(dayKey, String(nextDay), { expirationTtl: 35 * 86400 }),
  ]);
}

async function handleStats(env: Env): Promise<Response> {
  const index = await loadIndexFromR2(env.R2);
  const ids = index ? index.themes.map((t) => t.id) : [];
  const days = last30DaysUtc();
  const themes: Record<string, { total: number; last30d: number }> = {};

  await Promise.all(
    ids.map(async (id) => {
      const [totalRaw, ...dayRaws] = await Promise.all([
        env.STATS.get(`total:${id}`),
        ...days.map((d) => env.STATS.get(`day:${d}:${id}`)),
      ]);
      const total = Number.parseInt(totalRaw ?? '0', 10) || 0;
      const last30d = dayRaws.reduce((sum, v) => sum + (Number.parseInt(v ?? '0', 10) || 0), 0);
      themes[id] = { total, last30d };
    }),
  );

  return new Response(
    JSON.stringify({ schemaVersion: 1, generatedAt: new Date().toISOString(), themes }),
    {
      headers: {
        ...CORS_HEADERS,
        'Content-Type': 'application/json; charset=utf-8',
        'Cache-Control': 'public, max-age=300',
      },
    },
  );
}

async function handleDownload(
  id: string,
  commit: string,
  env: Env,
  ctx: ExecutionContext,
): Promise<Response> {
  if (!ID_RE.test(id) || !COMMIT_RE.test(commit)) {
    return new Response('Invalid theme id or commit', { status: 400, headers: CORS_HEADERS });
  }

  const index = await loadIndexFromR2(env.R2);
  const entry = index?.themes.find((t) => t.id === id && t.commit === commit);
  if (!entry || !entry.download?.url) {
    return new Response('Theme release not found in gallery index', {
      status: 404,
      headers: CORS_HEADERS,
    });
  }

  // Record anonymous count without delaying the response.
  ctx.waitUntil(incrementAnonymousCount(env.STATS, id));

  // Wallpaper-free themes mirrored in R2 are served directly from edge storage.
  const r2Archive = await env.R2.get(`gallery/archives/${id}-${commit}.tar.gz`);
  if (r2Archive) {
    return new Response(r2Archive.body, {
      headers: {
        ...CORS_HEADERS,
        'Content-Type': 'application/gzip',
        'Cache-Control': 'public, max-age=31536000, immutable',
        ETag: r2Archive.httpEtag,
      },
    });
  }

  // Wallpaper themes redirect directly to the author's pinned immutable GitHub archive.
  return new Response(null, {
    status: 302,
    headers: {
      ...CORS_HEADERS,
      Location: entry.download.url,
      'Cache-Control': 'no-store',
    },
  });
}

async function serveR2Object(
  key: string,
  contentType: string,
  cacheControl: string,
  env: Env,
  request: Request,
): Promise<Response> {
  const obj = await env.R2.get(key);
  if (!obj) {
    return new Response('Not Found', { status: 404, headers: CORS_HEADERS });
  }
  const ifNoneMatch = request.headers.get('If-None-Match');
  if (ifNoneMatch && ifNoneMatch === obj.httpEtag) {
    return new Response(null, {
      status: 304,
      headers: { ...CORS_HEADERS, ETag: obj.httpEtag, 'Cache-Control': cacheControl },
    });
  }
  return new Response(obj.body, {
    headers: {
      ...CORS_HEADERS,
      'Content-Type': contentType,
      ETag: obj.httpEtag,
      'Cache-Control': cacheControl,
    },
  });
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return new Response('Method Not Allowed', { status: 405, headers: CORS_HEADERS });
    }

    const url = new URL(request.url);
    const pathname = url.pathname;

    if (pathname === '/gallery/stats.json') {
      return handleStats(env);
    }

    if (pathname === '/gallery/index.json') {
      return serveR2Object(
        'gallery/index.json',
        'application/json; charset=utf-8',
        'public, max-age=300',
        env,
        request,
      );
    }

    if (pathname === '/gallery/index.json.sha256') {
      return serveR2Object(
        'gallery/index.json.sha256',
        'text/plain; charset=utf-8',
        'public, max-age=300',
        env,
        request,
      );
    }

    if (pathname === '/gallery/updates.json') {
      return serveR2Object(
        'gallery/updates.json',
        'application/json; charset=utf-8',
        'public, max-age=600',
        env,
        request,
      );
    }

    const thumbMatch = pathname.match(/^\/gallery\/thumbs\/([a-z0-9][a-z0-9-]{2,47}-[0-9a-f]{7,12}\.webp)$/i);
    if (thumbMatch) {
      return serveR2Object(
        `gallery/thumbs/${thumbMatch[1]}`,
        'image/webp',
        'public, max-age=31536000, immutable',
        env,
        request,
      );
    }

    const dlMatch = pathname.match(/^\/gallery\/dl\/([^/]+)\/([^/]+)$/);
    if (dlMatch) {
      return handleDownload(dlMatch[1], dlMatch[2], env, ctx);
    }

    return new Response('Not Found', { status: 404, headers: CORS_HEADERS });
  },
};
