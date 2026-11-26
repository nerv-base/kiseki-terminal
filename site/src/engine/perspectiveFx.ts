import type { ThemeId, ThemesBundle } from '../types/theme';

type ParticleItem =
  | {
      kind: 'petal';
      x: number;
      y: number;
      z: number;
      vx: number;
      vy: number;
      vz: number;
      rx: number;
      ry: number;
      rz: number;
      vrx: number;
      vry: number;
      vrz: number;
      size: number;
      phase: number;
    }
  | {
      kind: 'firefly';
      x: number;
      y: number;
      z: number;
      phase: number;
      speed: number;
    }
  | {
      kind: 'rain';
      x: number;
      y: number;
      z: number;
      len: number;
      speed: number;
    }
  | {
      kind: 'bokeh';
      x: number;
      y: number;
      z: number;
      r: number;
      warm: boolean;
      phase: number;
    }
  | {
      kind: 'hex';
      x: number;
      y: number;
      z: number;
      rotY: number;
      rotZ: number;
      vRot: number;
      radius: number;
      phase: number;
    }
  | {
      kind: 'pstar';
      x: number;
      y: number;
      z: number;
      vz: number;
      tint: string;
      phase: number;
    };

interface Meteor {
  x: number;
  y: number;
  life: number;
  maxLife: number;
}

export class Kiseki3DStage {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null;
  private bundle: ThemesBundle;
  private prefersReducedMotion: boolean;
  private isLandingActive: () => boolean;
  private themeId: ThemeId = 'starlight-8bit';
  private active = true;
  private isVisible = true;
  private rafId: number | null = null;
  private items: ParticleItem[] = [];
  private meteors: Meteor[] = [];
  private burstEnergy = 0;
  private mouseX = -9999;
  private mouseY = -9999;
  private fov = 520;
  private lastTime = performance.now();
  private w = 1200;
  private h = 720;
  private dpr = 1;
  private observer: IntersectionObserver | null = null;
  private resizeObs: ResizeObserver | null = null;

  constructor(
    canvas: HTMLCanvasElement,
    bundle: ThemesBundle,
    prefersReducedMotion: boolean,
    isLandingActive: () => boolean,
  ) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.bundle = bundle;
    this.prefersReducedMotion = prefersReducedMotion;
    this.isLandingActive = isLandingActive;

    if ('IntersectionObserver' in window) {
      this.observer = new IntersectionObserver(
        (entries) => {
          this.isVisible = entries[0]?.isIntersecting ?? true;
          if (this.isVisible) this.ensureLoop();
          else this.stopLoop();
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

  syncSize(): void {
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width < 10 || rect.height < 10) return;
    const rawDpr = Math.min(window.devicePixelRatio || 1, 2.0);
    const rawW = rect.width * rawDpr;
    const rawH = rect.height * rawDpr;
    const capScale = Math.max(rawW, rawH) > 3840 ? 3840 / Math.max(rawW, rawH) : 1;
    const cw = Math.max(1, Math.round(rawW * capScale));
    const ch = Math.max(1, Math.round(rawH * capScale));
    this.canvas.width = cw;
    this.canvas.height = ch;
    this.w = rect.width;
    this.h = rect.height;
    this.dpr = cw / rect.width;
  }

  setMouse(nx: number, ny: number): void {
    this.mouseX = nx;
    this.mouseY = ny;
  }

  triggerBurst(): void {
    this.burstEnergy = 1.0;
  }

  configure(themeId: ThemeId, isLive: boolean): void {
    const changed = this.themeId !== themeId;
    this.themeId = themeId;
    this.active = isLive && !this.prefersReducedMotion;
    this.syncSize();
    if (changed || this.items.length === 0) {
      this.spawnWorld(themeId);
    }
    if (!this.active) {
      this.stopLoop();
      this.ctx?.clearRect(0, 0, this.canvas.width, this.canvas.height);
    } else {
      this.ensureLoop();
    }
  }

  private spawnWorld(themeId: ThemeId): void {
    this.items = [];
    this.meteors = [];
    const w = this.w || 1200;
    const h = this.h || 720;

    if (themeId === 'kyoto-sakura') {
      for (let i = 0; i < 42; i++) {
        this.items.push({
          kind: 'petal',
          x: (Math.random() - 0.5) * w * 1.4,
          y: (Math.random() - 0.5) * h * 1.4,
          z: 40 + Math.random() * 680,
          vx: -18 - Math.random() * 24,
          vy: 24 + Math.random() * 28,
          vz: (Math.random() - 0.5) * 18,
          rx: Math.random() * Math.PI * 2,
          ry: Math.random() * Math.PI * 2,
          rz: Math.random() * Math.PI * 2,
          vrx: (Math.random() - 0.5) * 2.2,
          vry: (Math.random() - 0.5) * 2.6,
          vrz: (Math.random() - 0.5) * 1.4,
          size: 7 + Math.random() * 7,
          phase: Math.random() * 6.28,
        });
      }
      for (let i = 0; i < 16; i++) {
        this.items.push({
          kind: 'firefly',
          x: (Math.random() - 0.3) * w * 1.1,
          y: (Math.random() - 0.5) * h * 0.9,
          z: 80 + Math.random() * 520,
          phase: Math.random() * 6.28,
          speed: 0.7 + Math.random() * 1.1,
        });
      }
    } else if (themeId === 'shinkai-rain') {
      for (let i = 0; i < 65; i++) {
        this.items.push({
          kind: 'rain',
          x: (Math.random() - 0.5) * w * 1.5,
          y: (Math.random() - 0.5) * h * 1.4,
          z: 40 + Math.random() * 680,
          len: 28 + Math.random() * 44,
          speed: 520 + Math.random() * 380,
        });
      }
      for (let i = 0; i < 16; i++) {
        this.items.push({
          kind: 'bokeh',
          x: (Math.random() - 0.1) * w * 0.8,
          y: (Math.random() - 0.5) * h * 0.9,
          z: 180 + Math.random() * 540,
          r: 18 + Math.random() * 26,
          warm: i % 3 === 0,
          phase: Math.random() * 6.28,
        });
      }
    } else if (themeId === 'nerv-eva') {
      for (let i = 0; i < 12; i++) {
        this.items.push({
          kind: 'hex',
          x: (Math.random() - 0.1) * w * 0.85,
          y: (Math.random() - 0.5) * h * 0.9,
          z: 100 + Math.random() * 560,
          rotY: Math.random() * Math.PI,
          rotZ: Math.random() * Math.PI,
          vRot: (Math.random() - 0.5) * 0.55,
          radius: 26 + Math.random() * 28,
          phase: Math.random() * 6.28,
        });
      }
    } else {
      for (let i = 0; i < 60; i++) {
        this.items.push({
          kind: 'pstar',
          x: (Math.random() - 0.5) * w * 1.5,
          y: (Math.random() - 0.65) * h * 1.2,
          z: 80 + Math.random() * 760,
          vz: 24 + Math.random() * 36,
          tint: i % 5 === 0 ? '#FF82B8' : i % 3 === 0 ? '#66F0FF' : '#F8FAFF',
          phase: Math.random() * 6.28,
        });
      }
    }
  }

  private project(x: number, y: number, z: number): { sx: number; sy: number; scale: number } {
    const scale = this.fov / (this.fov + z);
    const cx = this.w * 0.5;
    const cy = this.h * 0.5;
    return { sx: cx + x * scale, sy: cy + y * scale, scale };
  }

  private step(now: number): void {
    const dt = Math.min(0.05, (now - this.lastTime) / 1000);
    this.lastTime = now;
    const t = now / 1000;
    const ctx = this.ctx;
    if (!ctx || !this.w || !this.h) return;

    ctx.save();
    ctx.setTransform(this.dpr || 1, 0, 0, this.dpr || 1, 0, 0);
    ctx.clearRect(0, 0, this.w, this.h);

    const pal = this.bundle.themes[this.themeId].tokens.palette;
    const acc1 = pal.accents.primary;
    const acc2 = pal.accents.secondary;
    const burst = this.burstEnergy;
    this.burstEnergy = Math.max(0, this.burstEnergy - dt * 1.8);
    const uiScale = Math.max(1, Math.min(1.6, Math.min(this.w, this.h * 1.778) / 1440));

    const halfW = this.w * 0.72;
    const halfH = this.h * 0.72;

    if (this.themeId === 'nerv-eva') {
      ctx.save();
      ctx.strokeStyle = acc1;
      const horizonY = this.h * 0.66;
      const gridY = 230;
      const offsetZ = (t * 65) % 80;

      for (let zi = 1; zi <= 11; zi++) {
        const z = zi * 80 - offsetZ;
        if (z < 20) continue;
        const s = this.fov / (this.fov + z);
        const py = horizonY + gridY * s * 0.38;
        ctx.globalAlpha = Math.max(0, 0.22 * (1 - zi / 12));
        ctx.lineWidth = uiScale;
        ctx.beginPath();
        ctx.moveTo(0, py);
        ctx.lineTo(this.w, py);
        ctx.stroke();
      }
      const cx = this.w * 0.62;
      for (let xi = -9; xi <= 9; xi++) {
        const wx = xi * 160;
        const pNearX = cx + wx * (this.fov / (this.fov + 40));
        const pFarX = cx + wx * (this.fov / (this.fov + 920));
        ctx.globalAlpha = 0.12;
        ctx.beginPath();
        ctx.moveTo(pNearX, this.h);
        ctx.lineTo(pFarX, horizonY + 12);
        ctx.stroke();
      }
      ctx.restore();
    }

    for (const p of this.items) {
      if (p.kind === 'petal') {
        p.x += (p.vx - burst * 280 + Math.sin(t * 1.3 + p.phase) * 14) * dt;
        p.y += (p.vy + burst * 40) * dt;
        p.z += p.vz * dt;
        p.rx += p.vrx * dt;
        p.ry += p.vry * dt;
        p.rz += p.vrz * dt;

        const proj = this.project(p.x, p.y, p.z);
        if (this.mouseX > 0) {
          const dx = proj.sx - this.mouseX;
          const dy = proj.sy - this.mouseY;
          const dist = Math.hypot(dx, dy);
          if (dist < 130 && dist > 1) {
            p.x += (dx / dist) * (130 - dist) * 0.55 * dt * 10;
            p.y += (dy / dist) * (130 - dist) * 0.55 * dt * 10;
          }
        }

        if (p.x < -halfW || p.y > halfH) {
          p.x = halfW * (0.2 + Math.random() * 0.8);
          p.y = -halfH;
          p.z = 40 + Math.random() * 680;
        }

        const pitchScale = Math.cos(p.rx);
        const rollScale = 0.45 + 0.55 * Math.abs(Math.sin(p.ry));
        const sz = p.size * proj.scale * uiScale;
        ctx.save();
        ctx.translate(proj.sx, proj.sy);
        ctx.rotate(p.rz);
        ctx.scale(rollScale, pitchScale);
        ctx.globalAlpha = Math.max(0.15, Math.min(0.88, (1 - p.z / 760) * 0.85));
        ctx.fillStyle = p.z < 180 ? '#FFD9EA' : acc1;
        ctx.beginPath();
        ctx.moveTo(0, -sz);
        ctx.bezierCurveTo(sz * 0.85, -sz * 0.4, sz * 0.7, sz * 0.65, 0, sz * 0.85);
        ctx.bezierCurveTo(-sz * 0.7, sz * 0.65, -sz * 0.85, -sz * 0.4, 0, -sz);
        ctx.fill();
        ctx.restore();
      } else if (p.kind === 'firefly') {
        const ox = Math.sin(t * p.speed + p.phase) * 32;
        const oy = Math.cos(t * p.speed * 0.8 + p.phase) * 24;
        const proj = this.project(p.x + ox, p.y + oy, p.z);
        const pulse = Math.pow(0.5 + 0.5 * Math.sin(t * 2.1 + p.phase), 2);
        const r = (2.0 + 2.2 * pulse) * proj.scale * uiScale;
        const alpha = 0.2 + 0.65 * pulse;
        ctx.fillStyle = acc2;
        ctx.globalAlpha = alpha * 0.22;
        ctx.beginPath();
        ctx.arc(proj.sx, proj.sy, r * 3.2, 0, Math.PI * 2);
        ctx.fill();
        ctx.globalAlpha = alpha;
        ctx.beginPath();
        ctx.arc(proj.sx, proj.sy, r, 0, Math.PI * 2);
        ctx.fill();
      } else if (p.kind === 'rain') {
        p.x -= (p.speed * 0.22 + burst * 220) * dt;
        p.y += p.speed * dt;
        if (p.y > halfH || p.x < -halfW) {
          p.y = -halfH;
          p.x = (Math.random() - 0.4) * this.w * 1.4;
        }
        const proj = this.project(p.x, p.y, p.z);
        const len = p.len * proj.scale * uiScale;
        ctx.strokeStyle = p.z < 220 ? acc1 : '#8FF5E5';
        ctx.globalAlpha = Math.max(0.08, (1 - p.z / 760) * 0.42);
        ctx.lineWidth = Math.max(0.7, 1.5 * proj.scale * uiScale);
        ctx.beginPath();
        ctx.moveTo(proj.sx, proj.sy);
        ctx.lineTo(proj.sx - len * 0.22, proj.sy + len);
        ctx.stroke();
      } else if (p.kind === 'bokeh') {
        const proj = this.project(p.x + Math.sin(t * 0.4 + p.phase) * 14, p.y, p.z);
        const pulse = 0.5 + 0.5 * Math.sin(t * 1.4 + p.phase);
        ctx.globalAlpha = 0.05 + 0.09 * pulse;
        ctx.fillStyle = p.warm ? acc2 : acc1;
        ctx.beginPath();
        ctx.arc(proj.sx, proj.sy, p.r * proj.scale * uiScale, 0, Math.PI * 2);
        ctx.fill();
      } else if (p.kind === 'hex') {
        p.rotZ += p.vRot * dt;
        p.rotY += p.vRot * 0.7 * dt;
        const proj = this.project(p.x + Math.sin(t * 0.6 + p.phase) * 16, p.y + Math.cos(t * 0.5 + p.phase) * 12, p.z);
        const r = p.radius * proj.scale * uiScale;
        ctx.save();
        ctx.translate(proj.sx, proj.sy);
        ctx.rotate(p.rotZ);
        ctx.scale(0.55 + 0.45 * Math.abs(Math.cos(p.rotY)), 1);
        ctx.strokeStyle = p.phase > 3.14 ? acc1 : acc2;
        ctx.globalAlpha = Math.max(0.12, (1 - p.z / 680) * 0.42);
        ctx.lineWidth = 1.2 * uiScale;
        ctx.beginPath();
        for (let k = 0; k < 6; k++) {
          const ang = (Math.PI / 3) * k;
          const vx = Math.cos(ang) * r;
          const vy = Math.sin(ang) * r;
          if (k === 0) ctx.moveTo(vx, vy);
          else ctx.lineTo(vx, vy);
        }
        ctx.closePath();
        ctx.stroke();
        ctx.restore();
      } else if (p.kind === 'pstar') {
        p.z -= (p.vz + burst * 240) * dt;
        if (p.z < 20) {
          p.z = 780;
          p.x = (Math.random() - 0.5) * this.w * 1.4;
          p.y = (Math.random() - 0.65) * this.h * 1.1;
        }
        const proj = this.project(p.x, p.y, p.z);
        const tw = 0.4 + 0.6 * Math.pow(0.5 + 0.5 * Math.sin(t * 2.8 + p.phase), 2);
        const px = Math.round(proj.sx / 2) * 2;
        const py = Math.round(proj.sy / 2) * 2;
        const sz = Math.max(2, Math.round(4 * proj.scale * uiScale));
        ctx.globalAlpha = Math.max(0.12, (1 - p.z / 800) * tw);
        ctx.fillStyle = p.tint;
        ctx.fillRect(px, py, sz, sz);
        if (tw > 0.85 && sz >= 3) {
          ctx.fillRect(px - sz, py, sz, sz);
          ctx.fillRect(px + sz, py, sz, sz);
          ctx.fillRect(px, py - sz, sz, sz);
          ctx.fillRect(px, py + sz, sz, sz);
        }
      }
    }

    if (this.themeId === 'starlight-8bit') {
      if (Math.random() < dt * 0.45 && this.meteors.length < 2) {
        this.meteors.push({
          x: this.w * (0.1 + Math.random() * 0.4),
          y: this.h * (0.06 + Math.random() * 0.22),
          life: 0,
          maxLife: 0.75,
        });
      }
      for (let i = this.meteors.length - 1; i >= 0; i--) {
        const m = this.meteors[i];
        if (!m) continue;
        m.life += dt;
        m.x += 340 * uiScale * dt;
        m.y += 190 * uiScale * dt;
        const alpha = Math.sin((m.life / m.maxLife) * Math.PI);
        ctx.strokeStyle = acc1;
        ctx.globalAlpha = Math.max(0, alpha * 0.85);
        ctx.lineWidth = 2 * uiScale;
        ctx.beginPath();
        ctx.moveTo(m.x, m.y);
        ctx.lineTo(m.x - 48 * uiScale, m.y - 27 * uiScale);
        ctx.stroke();
        if (m.life >= m.maxLife) this.meteors.splice(i, 1);
      }
    }

    ctx.restore();
  }

  ensureLoop(): void {
    if (this.rafId || !this.active || !this.isVisible || !this.isLandingActive()) return;
    this.lastTime = performance.now();
    const tick = (now: number) => {
      if (!this.active || !this.isVisible || !this.isLandingActive()) {
        this.rafId = null;
        return;
      }
      this.step(now);
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
  }
}
