interface R2RangeLike {
  offset?: number;
  length?: number;
  suffix?: number;
}

interface R2ObjectLike {
  key: string;
  size: number;
  httpEtag: string;
  range?: R2RangeLike;
  writeHttpMetadata(headers: Headers): void;
}

interface R2ObjectBodyLike extends R2ObjectLike {
  body: ReadableStream<Uint8Array>;
}

interface R2BucketLike {
  head(key: string): Promise<R2ObjectLike | null>;
  get(
    key: string,
    options?: { range?: Headers; onlyIf?: Headers },
  ): Promise<R2ObjectLike | R2ObjectBodyLike | null>;
}

interface Env {
  BUCKET: R2BucketLike;
  CN_ORIGIN: string;
}

interface CfRequest extends Request {
  cf?: { country?: string };
}

const MUTABLE_KEYS = new Set([
  'install.sh',
  'install.ps1',
  'releases/latest.json',
  'themes/index.json',
]);

function defaultCacheControl(key: string): string {
  return MUTABLE_KEYS.has(key)
    ? 'public, max-age=60'
    : 'public, max-age=31536000, immutable';
}

function defaultContentType(key: string): string {
  if (key.endsWith('.sh') || key.endsWith('.ps1') || key.endsWith('.txt') || key.endsWith('.tsv')) {
    return 'text/plain; charset=utf-8';
  }
  if (key.endsWith('.json')) return 'application/json; charset=utf-8';
  if (key.endsWith('.tar.gz') || key.endsWith('.tgz')) return 'application/gzip';
  if (key.endsWith('.zip')) return 'application/zip';
  return 'application/octet-stream';
}

function setCommonHeaders(headers: Headers, route: string): void {
  headers.set('Vary', 'CF-IPCountry');
  headers.set('X-Kiseki-Route', route);
  headers.set('Access-Control-Allow-Origin', '*');
  headers.set('Access-Control-Allow-Methods', 'GET, HEAD, OPTIONS');
}

function formatContentRange(range: R2RangeLike, totalSize: number): string | null {
  if (totalSize <= 0) return null;
  if (typeof range.offset === 'number' && typeof range.length === 'number' && range.length > 0) {
    const end = Math.min(totalSize - 1, range.offset + range.length - 1);
    return `bytes ${range.offset}-${end}/${totalSize}`;
  }
  if (typeof range.suffix === 'number' && range.suffix > 0) {
    const start = Math.max(0, totalSize - range.suffix);
    return `bytes ${start}-${totalSize - 1}/${totalSize}`;
  }
  return null;
}

export default {
  async fetch(request: CfRequest, env: Env): Promise<Response> {
    if (request.method === 'OPTIONS') {
      const headers = new Headers();
      setCommonHeaders(headers, 'cors');
      return new Response(null, { status: 204, headers });
    }

    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return new Response('Method Not Allowed', { status: 405 });
    }

    const url = new URL(request.url);
    const isHandledPath =
      url.pathname === '/install.sh' ||
      url.pathname === '/install.ps1' ||
      url.pathname.startsWith('/releases/') ||
      url.pathname.startsWith('/deps/') ||
      url.pathname.startsWith('/themes/');
    if (!isHandledPath) {
      return fetch(request);
    }
    const mirrorParam = url.searchParams.get('mirror');
    const country = request.cf?.country ?? '';
    const useCn = mirrorParam === 'cn' || (mirrorParam !== 'global' && country === 'CN');

    if (useCn) {
      const target = `${env.CN_ORIGIN.replace(/\/+$/, '')}${url.pathname}`;
      const headers = new Headers({
        Location: target,
        'Cache-Control': 'no-store',
      });
      setCommonHeaders(headers, 'cn-edgeone');
      return new Response(null, { status: 302, headers });
    }

    let key: string;
    try {
      key = decodeURIComponent(url.pathname.replace(/^\/+/, ''));
    } catch {
      return new Response('Bad Request', { status: 400 });
    }

    if (!key || key.includes('..') || key.includes('\\')) {
      return new Response('Bad Request', { status: 400 });
    }

    if (request.method === 'HEAD') {
      const head = await env.BUCKET.head(key);
      if (!head) return new Response('Not Found', { status: 404 });

      const headers = new Headers();
      head.writeHttpMetadata(headers);
      if (!headers.has('Content-Type')) headers.set('Content-Type', defaultContentType(key));
      if (!headers.has('Cache-Control')) headers.set('Cache-Control', defaultCacheControl(key));
      headers.set('ETag', head.httpEtag);
      headers.set('Content-Length', String(head.size));
      headers.set('Accept-Ranges', 'bytes');
      setCommonHeaders(headers, 'global-r2');
      return new Response(null, { status: 200, headers });
    }

    const obj = await env.BUCKET.get(key, {
      range: request.headers,
      onlyIf: request.headers,
    });
    if (!obj) return new Response('Not Found', { status: 404 });

    const headers = new Headers();
    obj.writeHttpMetadata(headers);
    if (!headers.has('Content-Type')) headers.set('Content-Type', defaultContentType(key));
    if (!headers.has('Cache-Control')) headers.set('Cache-Control', defaultCacheControl(key));
    headers.set('ETag', obj.httpEtag);
    headers.set('Accept-Ranges', 'bytes');
    setCommonHeaders(headers, 'global-r2');

    if (!('body' in obj)) {
      return new Response(null, { status: 304, headers });
    }

    if (request.headers.has('range') && obj.range) {
      const contentRange = formatContentRange(obj.range, obj.size);
      if (contentRange) {
        headers.set('Content-Range', contentRange);
        if (typeof obj.range.length === 'number') {
          headers.set('Content-Length', String(obj.range.length));
        }
        return new Response(obj.body, { status: 206, headers });
      }
    }

    headers.set('Content-Length', String(obj.size));
    return new Response(obj.body, { status: 200, headers });
  },
};
