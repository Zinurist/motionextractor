import type { Info, JobStatus, Listing, Preset, SessionInfo, Settings, VideoState } from './types';

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: 'same-origin',
  });
  const text = await res.text();
  const data = text ? JSON.parse(text) : null;
  if (!res.ok) {
    throw new ApiError(res.status, data?.error ?? `${res.status} ${res.statusText}`);
  }
  return data as T;
}

/**
 * Uploads a file into `dir` on the server (server mode). XMLHttpRequest instead of fetch, because
 * only it reports upload progress.
 */
export function uploadFile(
  dir: string,
  file: File,
  name: string,
  onProgress: (fraction: number) => void,
): Promise<{ path: string; size: number }> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    const q = new URLSearchParams({ dir, name });
    xhr.open('POST', `/api/fs/upload?${q}`);
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) onProgress(e.loaded / e.total);
    };
    xhr.onload = () => {
      const data = xhr.responseText ? JSON.parse(xhr.responseText) : null;
      if (xhr.status >= 200 && xhr.status < 300) resolve(data);
      else reject(new ApiError(xhr.status, data?.error ?? `upload failed (${xhr.status})`));
    };
    xhr.onerror = () => reject(new ApiError(0, 'upload failed: connection error'));
    xhr.send(file);
  });
}

/** URL that downloads a file from the server as an attachment. */
export function downloadUrl(path: string): string {
  return `/api/fs/download?path=${encodeURIComponent(path)}`;
}

/** A file name not yet used in `taken` ("clip.mp4" → "clip (2).mp4"). */
export function uniqueName(name: string, taken: Set<string>): string {
  if (!taken.has(name)) return name;
  const dot = name.lastIndexOf('.');
  const [stem, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
  for (let i = 2; ; i++) {
    const candidate = `${stem} (${i})${ext}`;
    if (!taken.has(candidate)) return candidate;
  }
}

export const api = {
  info: () => request<Info>('GET', '/info'),
  session: () => request<SessionInfo>('GET', '/session'),
  open: (path: string) => request<VideoState>('POST', '/open', { path }),
  patchPreset: (patch: unknown) =>
    request<{ preset: Preset; rev: number }>('PATCH', '/preset', patch),
  putPreset: (preset: Preset) => request<{ preset: Preset; rev: number }>('PUT', '/preset', preset),
  savePreset: (path: string) => request<{ path: string }>('POST', '/preset/save', { path }),
  loadPreset: (path: string) =>
    request<{ preset: Preset; rev: number }>('POST', '/preset/load', { path }),
  settings: () => request<Settings>('GET', '/settings'),
  putSettings: (s: Settings) => request<Settings>('PUT', '/settings', s),
  renderStatus: () => request<JobStatus | null>('GET', '/render'),
  startRender: (output: string, overwrite: boolean) =>
    request<JobStatus>('POST', '/render', { output, overwrite }),
  cancelRender: () => request<{ cancelled: boolean }>('DELETE', '/render'),
  renderCommand: (output: string) =>
    request<{ command: string }>('POST', '/render/command', { output }),
  list: (path?: string) =>
    request<Listing>('GET', `/fs/list${path ? `?path=${encodeURIComponent(path)}` : ''}`),
  dialog: (kind: 'open-video' | 'save-video' | 'open-preset' | 'save-preset', suggested = '') =>
    request<{ path: string | null }>('POST', `/dialog/${kind}`, { suggested }),
};
