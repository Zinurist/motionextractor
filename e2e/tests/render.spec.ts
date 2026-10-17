import { existsSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import { expect, makeSquareVideo, openVideo, probeFrames, probeSize, test } from './fixtures';

async function openRender(app: Page) {
  await app.getByTestId('open-render').click();
  const dialog = app.getByTestId('render-dialog');
  await expect(dialog).toBeVisible();
  return dialog;
}

const command = (app: Page) => app.getByTestId('render-command');

test('shows defaults and the exact ffmpeg command', async ({ app, server }) => {
  await openRender(app);
  await expect(app.getByTestId('render-output')).toHaveValue(join(server.dir, 'square_motion.mp4'));
  await expect(command(app)).toContainText('-c:v libx264 -crf 18 -preset medium');
  await expect(command(app)).toContainText('-c:a aac');
  await expect(command(app)).toContainText('square_motion.mp4');

  await app.getByTestId('render-codec').selectOption('vp9');
  await expect(app.getByTestId('render-output')).toHaveValue(
    join(server.dir, 'square_motion.webm'),
  );
  await expect(command(app)).toContainText('libvpx-vp9');
  await expect(command(app)).toContainText('-c:a libopus');
  await app.getByTestId('render-crf').fill('40');
  await app.getByTestId('render-crf').press('Enter');
  await expect(command(app)).toContainText('-crf 40');
  await app.getByTestId('render-encoder-preset').selectOption('4');
  await expect(command(app)).toContainText('-cpu-used 4');

  await app.getByTestId('render-codec').selectOption('h264');
  await expect(app.getByTestId('render-output')).toHaveValue(join(server.dir, 'square_motion.mp4'));
  await app.getByTestId('render-pix-fmt').fill('yuv444p');
  await app.getByTestId('render-pix-fmt').press('Enter');
  await expect(command(app)).toContainText('format=yuv444p');
  await app.getByTestId('render-extra-args').fill('-tune grain');
  await app.getByTestId('render-extra-args').press('Enter');
  await expect(command(app)).toContainText('-tune grain');
  await app.getByTestId('render-audio').selectOption('none');
  await expect(command(app)).not.toContainText('-c:a');

  // Range: frames 5 to 15 → the audio input is cut to match.
  await app.getByTestId('render-start').fill('5');
  await app.getByTestId('render-start').press('Enter');
  await app.getByTestId('render-end').fill('15');
  await app.getByTestId('render-end').press('Enter');
  await app.getByTestId('render-audio').selectOption('encode');
  await expect(command(app)).toContainText('-ss 0.166667 -t 0.333333');

  // Settings are part of the session (and of presets).
  const { preset } = await server.api<{ preset: { render: Record<string, unknown> } }>(
    'GET',
    '/api/preset',
  );
  expect(preset.render).toMatchObject({
    codec: 'h264',
    pix_fmt: 'yuv444p',
    extra_args: ['-tune', 'grain'],
    start: 5,
    end: 15,
  });
});

test('renders with the defaults', async ({ app, server }) => {
  await openRender(app);
  await app.getByTestId('render-start-button').click();
  const job = app.getByTestId('render-job');
  await expect(job).toHaveAttribute('data-state', 'finished');
  await expect(app.getByTestId('render-status')).toContainText('Finished');
  const out = join(server.dir, 'square_motion.mp4');
  expect(existsSync(out)).toBe(true);
  expect(probeFrames(out)).toBe(30);

  // Rendering again asks before overwriting.
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-confirm')).toBeVisible();
  await app.getByTestId('render-overwrite').click();
  await expect(app.getByTestId('render-confirm')).toBeHidden();
  await expect(job).toHaveAttribute('data-state', 'finished');
});

test('keeps an existing file unless confirmed', async ({ app, server }) => {
  const out = join(server.dir, 'square_motion.mp4');
  writeFileSync(out, 'keep');
  await openRender(app);
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-confirm')).toBeVisible();
  await app.getByRole('button', { name: 'Keep' }).click();
  await expect(app.getByTestId('render-confirm')).toBeHidden();
  expect(probeFramesSafe(out)).toBe(-1);
});

function probeFramesSafe(path: string): number {
  try {
    return probeFrames(path);
  } catch {
    return -1;
  }
}

test('renders a range at a lower resolution', async ({ app, server }) => {
  const big = makeSquareVideo(join(server.dir, 'big.mkv'), { width: 320, height: 240 });
  await openVideo(app, server, big, 30);
  await openRender(app);
  await app.getByTestId('render-height').selectOption('120');
  await app.getByTestId('render-start').fill('5');
  await app.getByTestId('render-start').press('Enter');
  await app.getByTestId('render-end').fill('15');
  await app.getByTestId('render-end').press('Enter');
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'finished');
  const out = join(server.dir, 'big_motion.mp4');
  expect(probeFrames(out)).toBe(10);
  expect(probeSize(out)).toEqual([160, 120]);
});

test('sets the range from the playhead', async ({ app }) => {
  await app.keyboard.press('ArrowRight');
  await app.keyboard.press('ArrowRight');
  await openRender(app);
  await app.getByTestId('render-start-current').click();
  await expect(app.getByTestId('render-start')).toHaveValue('2');
  await app.getByTestId('render-end-current').click();
  await expect(app.getByTestId('render-end')).toHaveValue('3');
  await app.getByTestId('render-range-clear').click();
  await expect(app.getByTestId('render-start')).toHaveValue('');
});

test('renders a PNG sequence', async ({ app, server }) => {
  await openRender(app);
  await app.getByTestId('render-codec').selectOption('png');
  await expect(app.getByTestId('render-output')).toHaveValue(join(server.dir, 'square_motion'));
  await expect(app.getByTestId('render-audio')).toBeDisabled();
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'finished');
  expect(readdirSync(join(server.dir, 'square_motion'))).toHaveLength(30);
});

test('chooses the output with the file browser', async ({ app, server }) => {
  await openRender(app);
  await app.getByTestId('render-browse').click();
  const browser = app.getByTestId('file-browser');
  await expect(app.getByTestId('fb-name')).toHaveValue('square_motion.mp4');
  await browser.locator('[data-name="clips"]').click();
  await app.getByTestId('fb-name').fill('mine.mp4');
  await app.getByTestId('fb-confirm').click();
  await expect(app.getByTestId('render-output')).toHaveValue(join(server.dir, 'clips', 'mine.mp4'));
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'finished');
  expect(existsSync(join(server.dir, 'clips', 'mine.mp4'))).toBe(true);
});

test('cancels a running render', async ({ app, server }) => {
  const long = makeSquareVideo(join(server.dir, 'long.mkv'), {
    frames: 600,
    width: 640,
    height: 480,
  });
  await openVideo(app, server, long, 600);
  await openRender(app);
  await app.getByTestId('render-encoder-preset').selectOption('veryslow');
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'running');
  await expect(app.getByTestId('render-start-button')).toBeDisabled();
  await app.getByTestId('render-cancel').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'cancelled');
  await expect(app.getByTestId('render-status')).toHaveText('Cancelled');
  expect(existsSync(join(server.dir, 'long_motion.mp4'))).toBe(false);
});

test('reports render errors', async ({ app }) => {
  await openRender(app);
  await app.getByTestId('render-extra-args').fill('-c:v no_such_encoder');
  await app.getByTestId('render-extra-args').press('Enter');
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'failed');
  await expect(app.getByTestId('render-status')).toContainText('no_such_encoder');
});

test('shows render progress outside the dialog', async ({ app, server }) => {
  const long = makeSquareVideo(join(server.dir, 'long.mkv'), {
    frames: 600,
    width: 640,
    height: 480,
  });
  await openVideo(app, server, long, 600);
  await openRender(app);
  await app.getByTestId('render-encoder-preset').selectOption('veryslow');
  await app.getByTestId('render-start-button').click();
  await app.getByTestId('render-close').click();
  await expect(app.getByTestId('render-progress-chip')).toContainText('Rendering');
  await app.getByTestId('render-progress-chip').click();
  await app.getByTestId('render-cancel').click();
  await expect(app.getByTestId('render-progress-chip')).toBeHidden();
});
