import type { FC } from 'react';

interface BootCurtainProps {
  done: boolean;
}

export const BootCurtain: FC<BootCurtainProps> = ({ done }) => (
  <div
    id="k-boot-curtain"
    className={`fixed inset-0 z-50 flex flex-col items-center justify-center gap-4 px-6 ${done ? 'is-done' : ''}`}
    style={{ background: 'var(--k-crust)' }}
  >
    <div
      className="font-k-display text-sm tracking-[0.34em] uppercase"
      style={{ color: 'var(--k-text-primary)', fontWeight: 'var(--k-display-weight)' }}
    >
      KISEKI <span className="font-k-mono text-[10px] opacity-55 tracking-[0.2em]">//</span>{' '}
      <span className="font-k-serif" style={{ color: 'var(--k-accent-primary)' }}>
        奇蹟端末
      </span>
    </div>
    <div
      className="w-44 h-[1px] overflow-hidden"
      style={{ background: 'color-mix(in srgb, var(--k-border) 35%, transparent)' }}
    >
      <div className="w-full h-full k-curtain-bar origin-center" style={{ background: 'var(--k-accent-primary)' }} />
    </div>
  </div>
);
