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
