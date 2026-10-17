import { test as base, expect, type Page } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { copyFileSync, mkdtempSync, mkdirSync, existsSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
import { createInterface } from 'node:readline';

/** The binary under test (built beforehand with `cargo build`). */
export const BIN =
  process.env.MX_BIN ??
  resolve(
    here,
    '../../target/debug/motion-extractor' + (process.platform === 'win32' ? '.exe' : ''),
  );

const FFMPEG = process.env.MOTION_EXTRACTOR_FFMPEG ?? 'ffmpeg';
const FFPROBE = process.env.MOTION_EXTRACTOR_FFPROBE ?? 'ffprobe';

/**
 * Same geometry as `mx_media::testing::MovingSquare`: a white 16×16 square moving 4 px per frame
 * over black, starting at x = 8, y = 40, losslessly encoded (exact pixel values).
 */
export const SQUARE = {
  width: 160,
  height: 120,
  fps: 30,
  size: 16,
  x0: 8,
  y: 40,
  speed: 4,
};
export const squareX = (n: number) => SQUARE.x0 + n * SQUARE.speed;
/** A row through the middle of the square. */
export const ROW = SQUARE.y + SQUARE.size / 2;

export function ffmpeg(args: string[]): void {
  try {
    // Single-threaded: many tests generate fixtures in parallel.
    execFileSync(FFMPEG, ['-hide_banner', '-v', 'error', '-y', '-filter_threads', '1', ...args], {
      stdio: 'pipe',
    });
  } catch (e) {
    const err = e as { stderr?: Buffer; status?: number; signal?: string };
    throw new Error(
      `ffmpeg failed (status ${err.status}, signal ${err.signal}): ${err.stderr?.toString() ?? ''}`,
    );
  }
}

export function makeSquareVideo(
  path: string,
  opts: {
    frames?: number;
    width?: number;
    height?: number;
    audio?: boolean;
  } = {},
): string {
  const frames = opts.frames ?? 30;
  const w = opts.width ?? SQUARE.width;
  const h = opts.height ?? SQUARE.height;
  const d = frames / SQUARE.fps + 1;
  const args = [
    '-f',
    'lavfi',
    '-i',
    `color=c=black:s=${w}x${h}:r=${SQUARE.fps}:d=${d}`,
    '-f',
    'lavfi',
    '-i',
    `color=c=white:s=${SQUARE.size}x${SQUARE.size}:r=${SQUARE.fps}:d=${d}`,
  ];
  if (opts.audio) args.push('-f', 'lavfi', '-i', `sine=frequency=440:duration=${d}`);
  args.push(
    '-filter_complex',
    `[0][1]overlay=x='${SQUARE.x0}+round(t*${SQUARE.fps})*${SQUARE.speed}':y=${SQUARE.y}:shortest=1:eval=frame,format=gbrp[v]`,
    '-map',
    '[v]',
  );
  if (opts.audio) args.push('-map', '2:a', '-c:a', 'flac');
  args.push('-frames:v', String(frames), '-c:v', 'ffv1', '-level', '3', '-threads', '1', path);
  ffmpeg(args);
  return path;
}

export function probeFrames(path: string): number {
  const out = execFileSync(FFPROBE, [
    '-v',
    'error',
    '-count_frames',
    '-select_streams',
    'v:0',
    '-show_entries',
    'stream=nb_read_frames',
    '-of',
    'csv=p=0',
    path,
  ]);
  return Number(out.toString().trim());
}

export interface Server {
  url: string;
  dir: string;
  video: string;
  proc: ChildProcess;
  api<T = unknown>(method: string, path: string, body?: unknown): Promise<T>;
}

async function startServer(dir: string, video: string): Promise<Server> {
  if (!existsSync(BIN)) throw new Error(`binary not found: ${BIN} (run cargo build first)`);
  const proc = spawn(
    BIN,
    ['serve', '--port', '0', '--root', dir, '--settings', join(dir, 'settings.json')],
    {
      stdio: ['ignore', 'pipe', 'inherit'],
    },
  );
  const url = await new Promise<string>((ok, fail) => {
    const rl = createInterface({ input: proc.stdout! });
    rl.once('line', (line) => ok(line.trim().split(' ').pop()!.replace(/\/$/, '')));
    proc.once('exit', (code) => fail(new Error(`server exited with ${code}`)));
  });
  const api = async <T>(method: string, path: string, body?: unknown): Promise<T> => {
    const res = await fetch(url + path, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text}`);
    return (text ? JSON.parse(text) : null) as T;
  };
  return { url, dir, video, proc, api };
}

type Fixtures = {
  /** A fresh server with its own root directory containing the square video. */
  server: Server;
  /** The app page with the square video opened. */
  app: Page;
};

type WorkerFixtures = {
  /** Videos generated once per worker. */
  media: { dir: string; square: string };
};

export const test = base.extend<Fixtures, WorkerFixtures>({
  media: [
    // eslint-disable-next-line no-empty-pattern
    async ({}, use) => {
      const dir = mkdtempSync(join(tmpdir(), 'mx-e2e-media-'));
      const square = makeSquareVideo(join(dir, 'square.mkv'), { audio: true });
      await use({ dir, square });
      rmSync(dir, { recursive: true, force: true });
    },
    { scope: 'worker' },
  ],

  server: async ({ media }, use) => {
    // The server reports canonical paths (no symlinks, long names on Windows).
    const dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'mx-e2e-')));
    mkdirSync(join(dir, 'clips'));
    const video = join(dir, 'square.mkv');
    copyFileSync(media.square, video);
    const server = await startServer(dir, video);
    await use(server);
    server.proc.kill();
    rmSync(dir, { recursive: true, force: true });
  },

  app: async ({ page, server }, use) => {
    await server.api('POST', '/api/open', { path: server.video });
    await page.goto(server.url + '/');
    await expect(page.getByTestId('connection')).toHaveAttribute('data-connected', 'true');
    await expectFrame(page, 0);
    await use(page);
  },
});

export { expect };

/** Waits until the viewer shows frame `index` exactly (optionally at parameter revision `rev`). */
export async function expectFrame(page: Page, index: number, rev?: number): Promise<void> {
  const viewer = page.getByTestId('viewer');
  await expect(viewer).toHaveAttribute('data-frame', String(index));
  await expect(viewer).toHaveAttribute('data-exact', 'true');
  if (rev !== undefined) await expect(viewer).toHaveAttribute('data-rev', String(rev));
}

/** Waits until the shown frame was computed with a revision newer than `rev`. */
export async function expectNewerRev(page: Page, rev: number): Promise<number> {
  const viewer = page.getByTestId('viewer');
  await expect.poll(async () => Number(await viewer.getAttribute('data-rev'))).toBeGreaterThan(rev);
  await expect(viewer).toHaveAttribute('data-exact', 'true');
  return Number(await viewer.getAttribute('data-rev'));
}

export async function currentRev(page: Page): Promise<number> {
  return Number(await page.getByTestId('viewer').getAttribute('data-rev'));
}

/** RGB of a pixel of the last received frame of `view`. */
export async function sample(
  page: Page,
  x: number,
  y: number,
  view: 'output' | 'original' = 'output',
): Promise<[number, number, number] | null> {
  return page.evaluate(
    ([v, px, py]) => window.mxDebug!.store.sample(v as 'output', px as number, py as number),
    [view, x, y] as const,
  );
}

/** Polls a pixel until it matches. */
export async function expectPixel(
  page: Page,
  x: number,
  y: number,
  rgb: [number, number, number],
  view: 'output' | 'original' = 'output',
): Promise<void> {
  await expect
    .poll(() => sample(page, x, y, view), {
      message: `pixel (${x}, ${y}) of ${view}`,
    })
    .toEqual(rgb);
}

/** Sets a slider/number pair through its number input. */
export async function setNumber(page: Page, testid: string, value: number | string): Promise<void> {
  const input = page.getByTestId(testid);
  await input.fill(String(value));
  await input.press('Enter');
  await input.blur();
}

declare global {
  interface Window {
    mxDebug?: {
      store: {
        sample(view: 'output' | 'original', x: number, y: number): [number, number, number] | null;
      };
    };
  }
}

/** Jumps to a frame through the frame field (and leaves the field so shortcuts work again). */
export async function goTo(page: Page, index: number): Promise<void> {
  await setNumber(page, 'frame-input', index);
  await expectFrame(page, index);
}

/** Width and height of the first video stream. */
export function probeSize(path: string): [number, number] {
  const out = execFileSync(FFPROBE, [
    '-v',
    'error',
    '-select_streams',
    'v:0',
    '-show_entries',
    'stream=width,height',
    '-of',
    'csv=p=0',
    path,
  ]);
  const [w, h] = out.toString().trim().split(',').map(Number);
  return [w, h];
}

/** Opens another video through the API and waits for the UI to show it. */
export async function openVideo(
  page: Page,
  server: Server,
  path: string,
  frames: number,
): Promise<void> {
  await server.api('POST', '/api/open', { path });
  await expect(page.getByTestId('video-name')).toContainText(`${frames} frames`);
  await expectFrame(page, 0);
}
