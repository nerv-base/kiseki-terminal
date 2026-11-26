import type { ShaderProfile, ThemeId, ThemesBundle } from '../types/theme';

interface LoadedWallpaper {
  img: HTMLImageElement;
  loaded: boolean;
  callbacks: Array<(img: HTMLImageElement) => void>;
}

const wallpaperCache = new Map<string, LoadedWallpaper>();

export function getWallpaperImage(
  themeId: ThemeId,
  bundle: ThemesBundle,
  onLoad?: (img: HTMLImageElement) => void,
): HTMLImageElement {
  const url = bundle.themes[themeId].wallpaperUrl;
  const cached = wallpaperCache.get(url);
  if (cached) {
    if (cached.loaded) {
      onLoad?.(cached.img);
    } else if (onLoad) {
      cached.callbacks.push(onLoad);
    }
    return cached.img;
  }

  const img = new Image();
  const entry: LoadedWallpaper = {
    img,
    loaded: false,
    callbacks: onLoad ? [onLoad] : [],
  };
  wallpaperCache.set(url, entry);

  img.onload = () => {
    const finish = () => {
      entry.loaded = true;
      for (const cb of entry.callbacks) cb(img);
      entry.callbacks = [];
    };
    if (typeof img.decode === 'function') {
      img.decode().then(finish).catch(finish);
    } else {
      finish();
    }
  };
  img.src = url;
  return img;
}

function hexToUnitVec3(hex: string): string {
  const clean = hex.replace('#', '');
  const r = parseInt(clean.slice(0, 2), 16) || 0;
  const g = parseInt(clean.slice(2, 4), 16) || 0;
  const b = parseInt(clean.slice(4, 6), 16) || 0;
  return `vec3(${(r / 255).toFixed(6)}, ${(g / 255).toFixed(6)}, ${(b / 255).toFixed(6)})`;
}

function toMacroCase(name: string): string {
  let out = '';
  for (let i = 0; i < name.length; i++) {
    const ch = name[i] ?? '';
    const prev = name[i - 1] ?? '';
    if (i > 0 && ch >= 'A' && ch <= 'Z' && prev >= 'a' && prev <= 'z') {
      out += '_';
    }
    out += ch.toUpperCase();
  }
  return out;
}

function toGlslFloat(val: number): string {
  return Number.isInteger(val) ? val.toFixed(1) : String(val);
}

const HELPERS_GLSL = `
float kiseki_bg_mask(vec4 term, vec2 uv) {
    vec3 unpremul = term.rgb / max(term.a, 0.001);
    float diff = length(unpremul - KISEKI_BASE_BG);
    float luma = dot(unpremul, vec3(0.299, 0.587, 0.114));
    float codeZone = 1.0 - smoothstep(KISEKI_SAFE_ZONE_END_X - 0.02, KISEKI_SAFE_ZONE_END_X + 0.18, uv.x);
    float glyph = smoothstep(0.22, 0.48, luma) * smoothstep(0.18, 0.42, diff);
    return 1.0 - glyph * mix(0.42, 0.88, codeZone);
}

float kiseki_narrow_factor(vec2 res) {
    float aspect = res.x / max(res.y, 1.0);
    return smoothstep(0.65, KISEKI_NARROW_FADE_ASPECT, aspect);
}
`;

interface ProgramEntry {
  program: WebGLProgram;
  uniforms: {
    iResolution: WebGLUniformLocation | null;
    iTime: WebGLUniformLocation | null;
    iChannel0: WebGLUniformLocation | null;
    u_wallSize: WebGLUniformLocation | null;
    u_bgOpacity: WebGLUniformLocation | null;
  };
}

interface TextureEntry {
  tex: WebGLTexture;
  width: number;
  height: number;
}

interface PendingUpdate {
  paletteId: ThemeId;
  atmosphereId: ThemeId;
  profile: ShaderProfile;
  bgOpacityOverride: number | null;
}

export class KisekiShaderStage {
  private canvas: HTMLCanvasElement;
  private mode: 'kv' | 'terminal';
  private bundle: ThemesBundle;
  private isLandingActive: () => boolean;
  private gl: WebGL2RenderingContext | null;
  private anisoExt: EXT_texture_filter_anisotropic | null = null;
  private maxAniso = 1;
  private programCache = new Map<string, ProgramEntry>();
  private texCache = new Map<ThemeId, TextureEntry>();
  private activeProgEntry: ProgramEntry | null = null;
  private activeTexEntry: TextureEntry | null = null;
  private vao: WebGLVertexArrayObject | null = null;
  private vbo: WebGLBuffer | null = null;
  private startTime = performance.now();
  private rafId: number | null = null;
  private isVisible = true;
  private lastWidth = 0;
  private lastHeight = 0;
  private pendingUpdate: PendingUpdate | null = null;
  private observer: IntersectionObserver | null = null;
  private resizeObs: ResizeObserver | null = null;

  private paletteId: ThemeId = 'starlight-8bit';
  private atmosphereId: ThemeId = 'starlight-8bit';
  private profile: ShaderProfile = 'balanced';
  private bgOpacity = 1.0;

  constructor(
    canvas: HTMLCanvasElement,
    mode: 'kv' | 'terminal',
    bundle: ThemesBundle,
    isLandingActive: () => boolean,
  ) {
    this.canvas = canvas;
    this.mode = mode;
    this.bundle = bundle;
    this.isLandingActive = isLandingActive;
    this.gl = canvas.getContext('webgl2', {
      alpha: false,
      antialias: false,
      powerPreference: 'high-performance',
    });

    if (this.gl) {
      this.anisoExt =
        this.gl.getExtension('EXT_texture_filter_anisotropic') ??
        this.gl.getExtension('WEBKIT_EXT_texture_filter_anisotropic');
      if (this.anisoExt) {
        this.maxAniso = this.gl.getParameter(this.anisoExt.MAX_TEXTURE_MAX_ANISOTROPY_EXT) as number;
      }
      this.initGL();
    }

    if ('IntersectionObserver' in window) {
      this.observer = new IntersectionObserver(
        (entries) => {
          this.isVisible = entries[0]?.isIntersecting ?? true;
          if (this.isVisible) {
            if (this.pendingUpdate) {
              const p = this.pendingUpdate;
              this.pendingUpdate = null;
              this.update(p.paletteId, p.atmosphereId, p.profile, p.bgOpacityOverride);
            } else {
              this.ensureLoop();
            }
          } else {
            this.stopLoop();
          }
        },
        { threshold: 0.05 },
      );
      this.observer.observe(this.canvas);
    }

    if ('ResizeObserver' in window) {
      this.resizeObs = new ResizeObserver(() => this.syncSize());
      this.resizeObs.observe(this.canvas);
    }
  }

  private initGL(): void {
    const gl = this.gl;
    if (!gl) return;
    const vao = gl.createVertexArray();
    gl.bindVertexArray(vao);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    this.vao = vao;
    this.vbo = buf;
  }

  private buildFragmentSource(paletteId: ThemeId, atmosphereId: ThemeId, profile: ShaderProfile): string {
    const pal = this.bundle.themes[paletteId].tokens;
    const atm = this.bundle.themes[atmosphereId].tokens;
    const qLevel = { static: 0, eco: 1, balanced: 2, ultra: 3 }[profile] ?? 2;
    const safeEnd = atm.atmosphere.wallpaper.codeSafeZone?.endX ?? 0.54;
    const narrowAspect = atm.atmosphere.shader.params?.narrowFadeAspect ?? 1.15;

    const lines = [
      '#version 300 es',
      'precision highp float;',
      'out vec4 kiseki_outColor;',
      'uniform vec3 iResolution;',
      'uniform float iTime;',
      'uniform sampler2D iChannel0;',
      'uniform vec2 u_wallSize;',
      'uniform float u_bgOpacity;',
      `#define KISEKI_QUALITY_LEVEL ${qLevel}`,
      `#define KISEKI_MODE_KV ${this.mode === 'kv' ? 1 : 0}`,
      `#define KISEKI_CRUST_BG ${hexToUnitVec3(pal.palette.surfaces.crust)}`,
      `#define KISEKI_BASE_BG ${hexToUnitVec3(pal.palette.surfaces.base)}`,
      `#define KISEKI_ACCENT_PRIMARY ${hexToUnitVec3(pal.palette.accents.primary)}`,
      `#define KISEKI_ACCENT_SECONDARY ${hexToUnitVec3(pal.palette.accents.secondary)}`,
      '#define KISEKI_BG_OPACITY 1.0000',
      `#define KISEKI_WINDOW_ALPHA ${(atm.atmosphere.window.opacity ?? 0.94).toFixed(4)}`,
      `#define KISEKI_SAFE_ZONE_END_X ${toGlslFloat(safeEnd)}`,
      `#define KISEKI_NARROW_FADE_ASPECT ${toGlslFloat(narrowAspect)}`,
      '#define KISEKI_ENABLE_CRT 0',
    ];

    const params = atm.atmosphere.shader.params ?? {};
    for (const [k, v] of Object.entries(params)) {
      lines.push(`#define KISEKI_PARAM_${toMacroCase(k)} ${toGlslFloat(v)}`);
    }

    lines.push(HELPERS_GLSL);
    lines.push(`
vec4 kiseki_sample_ch0(vec2 uv) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 wSize = max(u_wallSize, vec2(1.0));
    float scale = max(res.x / wSize.x, res.y / wSize.y);
    vec2 drawSize = wSize * scale;
    vec2 drawOrigin = res - drawSize;
    vec2 wuv = (uv * res - drawOrigin) / max(drawSize, vec2(1.0));
    vec3 wall = texture(iChannel0, clamp(wuv, 0.0, 1.0)).rgb;

#if KISEKI_MODE_KV == 1
    float leftVeil = 1.0 - smoothstep(0.0, 0.46, uv.x);
    leftVeil = leftVeil * leftVeil * 0.52;
    float botVeil = smoothstep(0.74, 1.0, uv.y);
    vec3 c = mix(wall, KISEKI_CRUST_BG, clamp(leftVeil, 0.0, 1.0));
    c = mix(c, KISEKI_CRUST_BG, botVeil);
    return vec4(c, 1.0);
#else
    float narrowFactor = kiseki_narrow_factor(res);
    float effAlpha = clamp(u_bgOpacity * (0.14 + 0.86 * narrowFactor), 0.0, 1.0);
    vec3 c = mix(KISEKI_BASE_BG, wall, effAlpha);
    float safeScrim = (1.0 - smoothstep(0.0, KISEKI_SAFE_ZONE_END_X + 0.16, uv.x)) * 0.88;
    c = mix(c, KISEKI_BASE_BG, safeScrim);
    return vec4(c, 1.0);
#endif
}
`);

    if (qLevel === 0) {
      lines.push(`
void main() {
    vec2 fragCoord = vec2(gl_FragCoord.x, iResolution.y - gl_FragCoord.y);
    vec2 uv = fragCoord / max(iResolution.xy, vec2(1.0));
    kiseki_outColor = vec4(kiseki_sample_ch0(uv).rgb, 1.0);
}
`);
    } else {
      const rewrittenGlsl = this.bundle.themes[atmosphereId].shaderGlsl.replace(
        /texture\(\s*iChannel0\s*,/g,
        'kiseki_sample_ch0(',
      );
      lines.push(rewrittenGlsl);
      lines.push(`
void main() {
    vec2 fragCoord = vec2(gl_FragCoord.x, iResolution.y - gl_FragCoord.y);
    vec4 col = vec4(0.0);
    mainImage(col, fragCoord);
    kiseki_outColor = vec4(col.rgb, 1.0);
}
`);
    }

    return lines.join('\n');
  }

  private compileShader(type: number, src: string): WebGLShader | null {
    const gl = this.gl;
    if (!gl) return null;
    const shader = gl.createShader(type);
    if (!shader) return null;
    gl.shaderSource(shader, src);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      console.warn('Kiseki shader compile error:', gl.getShaderInfoLog(shader));
      gl.deleteShader(shader);
      return null;
    }
    return shader;
  }

  private getOrCreateProgram(paletteId: ThemeId, atmosphereId: ThemeId, profile: ShaderProfile): ProgramEntry | null {
    const gl = this.gl;
    if (!gl) return null;
    const key = `${paletteId}|${atmosphereId}|${profile}`;
    const existing = this.programCache.get(key);
    if (existing) return existing;

    const vertSrc = `#version 300 es
layout(location = 0) in vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }`;
    const fragSrc = this.buildFragmentSource(paletteId, atmosphereId, profile);
    const vs = this.compileShader(gl.VERTEX_SHADER, vertSrc);
    const fs = this.compileShader(gl.FRAGMENT_SHADER, fragSrc);
    if (!vs || !fs) {
      if (vs) gl.deleteShader(vs);
      if (fs) gl.deleteShader(fs);
      return null;
    }

    const program = gl.createProgram();
    if (!program) {
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      return null;
    }
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    gl.deleteShader(vs);
    gl.deleteShader(fs);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      gl.deleteProgram(program);
      return null;
    }

    const entry: ProgramEntry = {
      program,
      uniforms: {
        iResolution: gl.getUniformLocation(program, 'iResolution'),
        iTime: gl.getUniformLocation(program, 'iTime'),
        iChannel0: gl.getUniformLocation(program, 'iChannel0'),
        u_wallSize: gl.getUniformLocation(program, 'u_wallSize'),
        u_bgOpacity: gl.getUniformLocation(program, 'u_bgOpacity'),
      },
    };
    this.programCache.set(key, entry);
    return entry;
  }

  private renderFallback2D(img: HTMLImageElement, paletteId: ThemeId, atmosphereId: ThemeId, bgOpacity: number): void {
    const ctx = this.canvas.getContext('2d');
    if (!ctx) return;
    const [w, h] = this.getTargetSize();
    this.canvas.width = w;
    this.canvas.height = h;
    const pal = this.bundle.themes[paletteId].tokens;
    ctx.fillStyle = this.mode === 'kv' ? pal.palette.surfaces.crust : pal.palette.surfaces.base;
    ctx.fillRect(0, 0, w, h);
    if (img.naturalWidth > 0) {
      const scale = Math.max(w / img.naturalWidth, h / img.naturalHeight);
      const dw = img.naturalWidth * scale;
      const dh = img.naturalHeight * scale;
      ctx.save();
      ctx.globalAlpha = this.mode === 'kv' ? 1.0 : bgOpacity;
      ctx.imageSmoothingEnabled = atmosphereId !== 'starlight-8bit';
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(img, w - dw, h - dh, dw, dh);
      ctx.restore();
    }
  }

  private ensureWallpaperTexture(atmosphereId: ThemeId, onReady: (entry: TextureEntry | null) => void): void {
    if (this.gl) {
      const cached = this.texCache.get(atmosphereId);
      if (cached) {
        onReady(cached);
        return;
      }
    }

    getWallpaperImage(atmosphereId, this.bundle, (img) => {
      if (!this.gl) {
        this.renderFallback2D(img, this.paletteId, atmosphereId, this.bgOpacity);
        onReady(null);
        return;
      }
      const existing = this.texCache.get(atmosphereId);
      if (existing) {
        onReady(existing);
        return;
      }
      const gl = this.gl;
      const tex = gl.createTexture();
      if (!tex) {
        onReady(null);
        return;
      }
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);

      if (atmosphereId === 'starlight-8bit') {
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      } else {
        gl.generateMipmap(gl.TEXTURE_2D);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        if (this.anisoExt) {
          gl.texParameterf(gl.TEXTURE_2D, this.anisoExt.TEXTURE_MAX_ANISOTROPY_EXT, Math.min(8, this.maxAniso));
        }
      }

      const entry: TextureEntry = { tex, width: img.naturalWidth, height: img.naturalHeight };
      this.texCache.set(atmosphereId, entry);
      onReady(entry);
    });
  }

  private getTargetSize(): [number, number] {
    const rect = this.canvas.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2.0);
    return [
      Math.max(64, Math.round((rect.width || 1024) * dpr)),
      Math.max(64, Math.round((rect.height || 640) * dpr)),
    ];
  }

  syncSize(): void {
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width < 10 || rect.height < 10) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2.0);
    const w = Math.round(rect.width * dpr);
    const h = Math.round(rect.height * dpr);
    if (w !== this.lastWidth || h !== this.lastHeight) {
      this.lastWidth = w;
      this.lastHeight = h;
      this.canvas.width = w;
      this.canvas.height = h;
      if (!this.gl) {
        getWallpaperImage(this.atmosphereId, this.bundle, (img) => {
          this.renderFallback2D(img, this.paletteId, this.atmosphereId, this.bgOpacity);
        });
      } else if (this.profile === 'static' && this.isVisible && this.isLandingActive()) {
        this.renderFrame();
      }
    }
  }

  prewarm(themeId: ThemeId, profile: ShaderProfile): void {
    if (!this.gl) return;
    this.getOrCreateProgram(themeId, themeId, profile);
    this.ensureWallpaperTexture(themeId, () => {});
  }

  flushPending(): void {
    if (this.pendingUpdate) {
      const p = this.pendingUpdate;
      this.pendingUpdate = null;
      this.update(p.paletteId, p.atmosphereId, p.profile, p.bgOpacityOverride);
    } else {
      this.ensureLoop();
    }
  }

  update(paletteId: ThemeId, atmosphereId: ThemeId, profile: ShaderProfile, bgOpacityOverride: number | null): void {
    if (this.mode === 'terminal' && (!this.isVisible || !this.isLandingActive())) {
      this.pendingUpdate = { paletteId, atmosphereId, profile, bgOpacityOverride };
      return;
    }

    const atm = this.bundle.themes[atmosphereId].tokens;
    const bgOpacity =
      this.mode === 'kv'
        ? 1.0
        : (bgOpacityOverride ?? atm.atmosphere.wallpaper.opacity ?? 0.45);

    this.paletteId = paletteId;
    this.atmosphereId = atmosphereId;
    this.profile = profile;
    this.bgOpacity = bgOpacity;

    const [w, h] = this.getTargetSize();
    if (w !== this.lastWidth || h !== this.lastHeight) {
      this.lastWidth = w;
      this.lastHeight = h;
      this.canvas.width = w;
      this.canvas.height = h;
    }

    this.activeProgEntry = this.getOrCreateProgram(paletteId, atmosphereId, profile);
    this.ensureWallpaperTexture(atmosphereId, (texEntry) => {
      if (this.atmosphereId === atmosphereId && texEntry) {
        this.activeTexEntry = texEntry;
        this.renderFrame();
      }
    });

    if (profile === 'static' || !this.isLandingActive()) {
      this.stopLoop();
      this.renderFrame();
    } else {
      this.ensureLoop();
    }
  }

  renderFrame(): void {
    const gl = this.gl;
    if (!gl || !this.activeProgEntry || !this.activeTexEntry) return;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.useProgram(this.activeProgEntry.program);
    gl.bindVertexArray(this.vao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.activeTexEntry.tex);
    gl.uniform1i(this.activeProgEntry.uniforms.iChannel0, 0);
    gl.uniform3f(this.activeProgEntry.uniforms.iResolution, this.canvas.width, this.canvas.height, 1.0);
    gl.uniform1f(this.activeProgEntry.uniforms.iTime, (performance.now() - this.startTime) / 1000.0);
    gl.uniform2f(this.activeProgEntry.uniforms.u_wallSize, this.activeTexEntry.width, this.activeTexEntry.height);
    gl.uniform1f(this.activeProgEntry.uniforms.u_bgOpacity, this.bgOpacity);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  ensureLoop(): void {
    if (this.rafId || this.profile === 'static' || !this.isVisible || !this.isLandingActive() || !this.gl) return;
    const tick = () => {
      if (!this.isVisible || this.profile === 'static' || !this.isLandingActive()) {
        this.rafId = null;
        return;
      }
      this.renderFrame();
      this.rafId = requestAnimationFrame(tick);
    };
    this.rafId = requestAnimationFrame(tick);
  }

  stopLoop(): void {
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }
  }

  dispose(): void {
    this.stopLoop();
    this.observer?.disconnect();
    this.resizeObs?.disconnect();
    if (this.gl) {
      for (const entry of this.programCache.values()) {
        this.gl.deleteProgram(entry.program);
      }
      for (const entry of this.texCache.values()) {
        this.gl.deleteTexture(entry.tex);
      }
      if (this.vbo) this.gl.deleteBuffer(this.vbo);
      if (this.vao) this.gl.deleteVertexArray(this.vao);
      this.programCache.clear();
      this.texCache.clear();
    }
  }
}
