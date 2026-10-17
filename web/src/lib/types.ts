// Mirrors of the backend JSON types (see CLAUDE.md §3.4).

export interface Rational {
  num: number;
  den: number;
}

export interface VideoInfo {
  path: string;
  width: number;
  height: number;
  fps: Rational;
  frame_count: number;
  duration: number;
  has_audio: boolean;
  codec: string;
  format: string;
  start_time: number;
}

export interface VideoState {
  info: VideoInfo;
  preview_width: number;
  preview_height: number;
}

export type TimeUnit = 'frames' | 'seconds';

export type Comparison =
  { mode: 'rolling'; offset: { value: number; unit: TimeUnit } } | { mode: 'fixed'; frame: number };

export interface Bloom {
  enabled: boolean;
  threshold: number;
  radius: number;
  intensity: number;
}

export interface EffectParams {
  blend: 'difference' | 'classic';
  metric: 'luma' | 'max';
  denoise: number;
  threshold: number;
  gain: number;
  gamma: number;
  color: string;
  motion_fill: 'color' | 'original';
  bloom: Bloom;
  background: 'color' | 'original';
  background_color: string;
  background_brightness: number;
  overlay_opacity: number;
}

export type Codec = 'h264' | 'h265' | 'vp9' | 'prores' | 'ffv1' | 'png';

export interface RenderOptions {
  codec: Codec;
  crf: number | null;
  encoder_preset: string | null;
  pix_fmt: string | null;
  audio: 'encode' | 'copy' | 'none';
  extra_args: string[];
  start: number | null;
  end: number | null;
  height: number | null;
}

export interface Preset {
  version: number;
  comparison: Comparison;
  effect: EffectParams;
  render: RenderOptions;
}

export interface CacheState {
  ranges: [number, number][];
  capacity: number;
  cached: number;
  decoding: boolean;
}

export interface JobStatus {
  id: number;
  state: 'running' | 'finished' | 'failed' | 'cancelled';
  done: number;
  total: number;
  fps: number;
  eta_seconds: number | null;
  elapsed_seconds: number;
  output: string;
  command: string;
  error: string | null;
}

export interface Settings {
  cache_max_memory_mb: number;
  preview_height: number;
  decode_workers: number;
  hwaccel: boolean;
  jpeg_quality: number;
  ffmpeg_path: string | null;
  ffprobe_path: string | null;
}

export interface Info {
  version: string;
  mode: 'desktop' | 'server';
  ffmpeg: { ok: boolean; version: string | null; error: string | null };
  roots: string[];
  dialogs: boolean;
}

export interface SessionInfo {
  video: VideoState | null;
  preset: Preset;
  rev: number;
  cache: CacheState;
  render: JobStatus | null;
  settings: Settings;
}

export interface Listing {
  path: string;
  parent: string | null;
  entries: { name: string; path: string; dir: boolean; size: number }[];
}

export type ServerEvent =
  | { type: 'opened'; video: VideoState }
  | { type: 'video'; video: VideoState }
  | { type: 'preset'; preset: Preset; rev: number }
  | { type: 'cache'; cache: CacheState }
  | { type: 'render'; job: JobStatus }
  | { type: 'settings'; settings: Settings }
  | { type: 'error'; message: string };

export type View = 'output' | 'original';

export const CODECS: { value: Codec; label: string; ext: string }[] = [
  { value: 'h264', label: 'H.264 (MP4)', ext: 'mp4' },
  { value: 'h265', label: 'H.265 (MP4)', ext: 'mp4' },
  { value: 'vp9', label: 'VP9 (WebM)', ext: 'webm' },
  { value: 'prores', label: 'ProRes (MOV)', ext: 'mov' },
  { value: 'ffv1', label: 'FFV1 lossless (MKV)', ext: 'mkv' },
  { value: 'png', label: 'PNG sequence', ext: '' },
];

export function defaultPreset(): Preset {
  return {
    version: 1,
    comparison: { mode: 'rolling', offset: { value: 1, unit: 'frames' } },
    effect: {
      blend: 'difference',
      metric: 'luma',
      denoise: 0,
      threshold: 0.02,
      gain: 4,
      gamma: 1,
      color: '#ffffff',
      motion_fill: 'color',
      bloom: { enabled: false, threshold: 0, radius: 0.02, intensity: 1 },
      background: 'color',
      background_color: '#000000',
      background_brightness: 1,
      overlay_opacity: 1,
    },
    render: {
      codec: 'h264',
      crf: null,
      encoder_preset: null,
      pix_fmt: null,
      audio: 'encode',
      extra_args: [],
      start: null,
      end: null,
      height: null,
    },
  };
}
