import { api, ApiError } from './api';
import { deepMerge } from './merge';
import type { FrameMessage } from './protocol';
import { FORMAT_RGB } from './protocol';
import { Socket } from './socket';
import { clamp, fpsValue } from './time';
import {
  defaultPreset,
  type CacheState,
  type Info,
  type JobStatus,
  type Preset,
  type ServerEvent,
  type SessionInfo,
  type Settings,
  type VideoState,
  type View,
} from './types';

export type ViewMode = 'output' | 'original' | 'split' | 'side';

export interface Shown {
  index: number;
  exact: boolean;
  rev: number;
  seq: number;
}

export interface Toast {
  id: number;
  kind: 'error' | 'info';
  text: string;
}

type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K] };

class AppStore {
  info = $state<Info | null>(null);
  video = $state<VideoState | null>(null);
  preset = $state<Preset>(defaultPreset());
  rev = $state(0);
  cache = $state<CacheState>({ ranges: [], capacity: 0, cached: 0, decoding: false });
  job = $state<JobStatus | null>(null);
  settings = $state<Settings | null>(null);
  connected = $state(false);
  frame = $state(0);
  playing = $state(false);
  viewMode = $state<ViewMode>('output');
  shown = $state<Record<View, Shown | null>>({ output: null, original: null });
  toasts = $state<Toast[]>([]);

  /** Latest raw frames (not reactive: large buffers). */
  frames: Record<View, FrameMessage | null> = { output: null, original: null };
  private frameListeners = new Set<(f: FrameMessage) => void>();
  private socket = new Socket();
  private patch: unknown = null;
  private patching = false;
  private lastSent = '';
  private toastId = 0;
  private playStart = { time: 0, frame: 0 };

  get fps(): number {
    return this.video ? fpsValue(this.video.info.fps) : 30;
  }

  get frameCount(): number {
    return this.video?.info.frame_count ?? 0;
  }

  get neededViews(): View[] {
    switch (this.viewMode) {
      case 'output':
        return ['output'];
      case 'original':
        return ['original'];
      default:
        return ['output', 'original'];
    }
  }

  async init(): Promise<void> {
    this.socket.onEvent = (e) => this.handleEvent(e);
    this.socket.onFrame = (f) => this.handleFrame(f);
    this.socket.onStatus = (c) => {
      this.connected = c;
      if (c) void this.reload();
    };
    this.socket.connect();
    try {
      this.info = await api.info();
      await this.reload();
    } catch (e) {
      this.error(e);
    }
  }

  private async reload(): Promise<void> {
    this.applySession(await api.session());
  }

  private applySession(s: SessionInfo): void {
    this.video = s.video;
    this.preset = s.preset;
    this.rev = s.rev;
    this.cache = s.cache;
    this.job = s.render;
    this.settings = s.settings;
    this.lastSent = JSON.stringify(s.preset);
    this.frame = clamp(this.frame, 0, Math.max(0, this.frameCount - 1));
    this.requestViews();
  }

  private handleEvent(e: ServerEvent): void {
    switch (e.type) {
      case 'opened':
        this.video = e.video;
        this.frames = { output: null, original: null };
        this.shown = { output: null, original: null };
        this.pause();
        this.frame = 0;
        this.requestViews();
        break;
      case 'preset': {
        const json = JSON.stringify(e.preset);
        // Our own changes come back too; only adopt foreign ones (another client, preset load).
        if (json !== this.lastSent && !this.patching && this.patch === null) {
          this.preset = e.preset;
          this.lastSent = json;
        }
        if (e.rev !== this.rev) {
          this.rev = e.rev;
          this.requestViews();
        }
        break;
      }
      case 'cache':
        this.cache = e.cache;
        break;
      case 'render':
        this.setJob(e.job);
        break;
      case 'settings':
        this.settings = e.settings;
        break;
      case 'error':
        this.toast('error', e.message);
        break;
    }
  }

  private handleFrame(f: FrameMessage): void {
    const shown = this.shown[f.view];
    if (shown && (f.seq < shown.seq || (f.seq === shown.seq && shown.exact && !f.exact))) return;
    this.frames[f.view] = f;
    this.shown[f.view] = { index: f.index, exact: f.exact, rev: f.rev, seq: f.seq };
    for (const listener of this.frameListeners) listener(f);
  }

  onFrame(listener: (f: FrameMessage) => void): () => void {
    this.frameListeners.add(listener);
    return () => this.frameListeners.delete(listener);
  }

  /** Asks the server for the frames the current view mode shows. */
  requestViews(): void {
    if (!this.video) return;
    for (const view of this.neededViews) this.socket.request(view, this.frame);
  }

  setViewMode(mode: ViewMode): void {
    this.viewMode = mode;
    this.requestViews();
  }

  setFrame(i: number): void {
    if (!this.video) return;
    const f = clamp(Math.round(i), 0, this.frameCount - 1);
    if (f !== this.frame) {
      this.frame = f;
      this.requestViews();
    }
  }

  step(delta: number): void {
    this.pause();
    this.setFrame(this.frame + delta);
  }

  /** The trimmed part of the video as frames `[start, end)` (the whole video when not trimmed). */
  get trim(): { start: number; end: number } {
    const count = this.frameCount;
    const end = clamp(this.preset.render.end ?? count, 1, Math.max(1, count));
    const start = clamp(this.preset.render.start ?? 0, 0, end - 1);
    return { start, end };
  }

  get trimmed(): boolean {
    const { start, end } = this.trim;
    return start > 0 || end < this.frameCount;
  }

  /** Sets the trim range (frames `[start, end)`); the full video is stored as "no trim". */
  setTrim(start: number, end: number): void {
    const count = this.frameCount;
    if (!count) return;
    const e = clamp(Math.round(end), 1, count);
    const s = clamp(Math.round(start), 0, e - 1);
    this.updatePreset({ render: { start: s > 0 ? s : null, end: e < count ? e : null } });
  }

  /** Jumps to the first frame of the trimmed part. */
  goStart(): void {
    this.pause();
    this.setFrame(this.trim.start);
  }

  /** Jumps to the last frame of the trimmed part. */
  goEnd(): void {
    this.pause();
    this.setFrame(this.trim.end - 1);
  }

  /** Plays the trimmed part from the playhead (or from its start) and stops at its end. */
  play(): void {
    if (!this.video || this.playing) return;
    const { start, end } = this.trim;
    if (this.frame < start || this.frame >= end - 1) this.setFrame(start);
    this.playing = true;
    this.playStart = { time: performance.now(), frame: this.frame };
    const tick = () => {
      if (!this.playing) return;
      const last = this.trim.end - 1;
      const elapsed = (performance.now() - this.playStart.time) / 1000;
      const target = this.playStart.frame + Math.floor(elapsed * this.fps);
      if (target >= last) {
        this.setFrame(last);
        this.playing = false;
        return;
      }
      this.setFrame(target);
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  pause(): void {
    this.playing = false;
  }

  togglePlay(): void {
    if (this.playing) this.pause();
    else this.play();
  }

  /** Applies a change to the preset locally and sends it (coalesced) to the server. */
  updatePreset(patch: DeepPartial<Preset>): void {
    this.preset = deepMerge(this.preset, patch);
    this.patch = this.patch === null ? patch : deepMerge(this.patch, patch);
    void this.flushPatch();
  }

  /** Replaces the comparison (a tagged union, so it is not merged). */
  setComparison(c: Preset['comparison']): void {
    this.preset = { ...this.preset, comparison: c };
    this.patch = { ...(this.patch as object | null), comparison: c };
    void this.flushPatch();
  }

  private async flushPatch(): Promise<void> {
    if (this.patching || this.patch === null) return;
    const patch = this.patch;
    this.patch = null;
    this.patching = true;
    try {
      const r = await api.patchPreset(patch);
      this.lastSent = JSON.stringify(r.preset);
      if (r.rev !== this.rev) {
        this.rev = r.rev;
        this.requestViews();
      }
    } catch (e) {
      this.error(e);
      // Resynchronise with the server's state.
      void this.reload();
    } finally {
      this.patching = false;
      if (this.patch !== null) void this.flushPatch();
    }
  }

  async replacePreset(preset: Preset): Promise<void> {
    try {
      const r = await api.putPreset(preset);
      this.preset = r.preset;
      this.lastSent = JSON.stringify(r.preset);
      this.rev = r.rev;
      this.requestViews();
    } catch (e) {
      this.error(e);
    }
  }

  async open(path: string): Promise<boolean> {
    try {
      this.video = await api.open(path);
      this.frame = 0;
      this.requestViews();
      return true;
    } catch (e) {
      this.error(e);
      return false;
    }
  }

  async saveSettings(s: Settings): Promise<boolean> {
    try {
      this.settings = await api.putSettings(s);
      return true;
    } catch (e) {
      this.error(e);
      return false;
    }
  }

  /**
   * Updates the render job unless the update is older than what is shown: a fast render can finish
   * (WebSocket event) before the response to the request that started it arrives.
   */
  setJob(job: JobStatus): void {
    const current = this.job;
    const stale =
      current !== null &&
      (job.id < current.id ||
        (job.id === current.id && current.state !== 'running' && job.state === 'running'));
    if (!stale) this.job = job;
  }

  toast(kind: Toast['kind'], text: string): void {
    const id = ++this.toastId;
    this.toasts = [...this.toasts, { id, kind, text }];
    setTimeout(() => this.dismiss(id), kind === 'error' ? 8000 : 4000);
  }

  dismiss(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  error(e: unknown): void {
    const text = e instanceof ApiError || e instanceof Error ? e.message : String(e);
    this.toast('error', text);
  }

  /** RGB of pixel (x, y) of the last received frame (raw frames only); used by E2E tests. */
  sample(view: View, x: number, y: number): [number, number, number] | null {
    const f = this.frames[view];
    if (!f || f.format !== FORMAT_RGB || x >= f.width || y >= f.height) return null;
    const i = (y * f.width + x) * 3;
    return [f.payload[i], f.payload[i + 1], f.payload[i + 2]];
  }
}

export const store = new AppStore();

declare global {
  interface Window {
    mxDebug?: { store: AppStore };
  }
}
if (typeof window !== 'undefined') window.mxDebug = { store };
