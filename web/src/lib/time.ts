import type { Rational, TimeUnit } from './types';

export function fpsValue(fps: Rational): number {
  return fps.num / fps.den;
}

/** Offset in whole frames, never zero for a non-zero value (mirrors `Offset::to_frames`). */
export function offsetFrames(value: number, unit: TimeUnit, fps: number): number {
  const raw = unit === 'frames' ? value : value * fps;
  const k = Math.round(raw);
  return k === 0 && raw !== 0 ? Math.sign(raw) : k;
}

/** `mm:ss.cc` (or `h:mm:ss.cc` from one hour). */
export function formatTime(seconds: number): string {
  const s = Math.max(0, seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const rest = s - h * 3600 - m * 60;
  const ss = rest.toFixed(2).padStart(5, '0');
  const mm = String(m).padStart(2, '0');
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null || !isFinite(seconds)) return '–';
  const s = Math.round(seconds);
  return s >= 60 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${s}s`;
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}
