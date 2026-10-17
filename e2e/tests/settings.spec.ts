import { join } from 'node:path';
import { expect, makeSquareVideo, openVideo, sample, test } from './fixtures';

test('memory estimate and persistence', async ({ app, server }) => {
  await app.getByTestId('open-settings').click();
  const dialog = app.getByTestId('settings-dialog');
  await expect(dialog).toBeVisible();
  await expect(app.getByTestId('settings-memory')).toHaveValue('2048');
  await expect(app.getByTestId('settings-memory-estimate')).toContainText('at 160×120');
  await expect(app.getByTestId('settings-memory-estimate')).toContainText('the whole video fits');
  await app.getByTestId('settings-memory').fill('64');
  // 64 MiB / (160 · 120 · 3 B) = 1165 frames
  await expect(app.getByTestId('settings-memory-estimate')).toContainText('≈ 1165 frames');
  await app.getByTestId('settings-workers').fill('3');
  await app.getByTestId('settings-hwaccel').check();
  await app.getByTestId('settings-save').click();
  await expect(dialog).toBeHidden();

  const settings = await server.api<Record<string, unknown>>('GET', '/api/settings');
  expect(settings).toMatchObject({ cache_max_memory_mb: 64, decode_workers: 3, hwaccel: true });
  // After a reload the dialog shows the saved values.
  await app.reload();
  await app.getByTestId('open-settings').click();
  await expect(app.getByTestId('settings-memory')).toHaveValue('64');
  await expect(app.getByTestId('settings-hwaccel')).toBeChecked();
  // Cancel discards edits.
  await app.getByTestId('settings-memory').fill('128');
  await app.getByRole('button', { name: 'Cancel' }).click();
  expect(
    (await server.api<{ cache_max_memory_mb: number }>('GET', '/api/settings')).cache_max_memory_mb,
  ).toBe(64);
});

test('preview resolution', async ({ app, server }) => {
  const big = makeSquareVideo(join(server.dir, 'big.mkv'), { width: 640, height: 480 });
  await openVideo(app, server, big, 30);
  // Default 540p is above the native 480: native size.
  await expect.poll(() => sample(app, 639, 479)).not.toBeNull();
  await app.getByTestId('open-settings').click();
  await app.getByTestId('settings-preview-height').selectOption('240');
  await expect(app.getByTestId('settings-memory-estimate')).toContainText('at 320×240');
  await app.getByTestId('settings-save').click();
  // The video is reopened at the new size.
  await expect.poll(() => sample(app, 320, 0)).toBeNull();
  await expect.poll(() => sample(app, 319, 239)).not.toBeNull();
  const session = await server.api<{ video: { preview_width: number } }>('GET', '/api/session');
  expect(session.video.preview_width).toBe(320);
});

test('rejects invalid settings', async ({ app }) => {
  await app.getByTestId('open-settings').click();
  await app.getByTestId('settings-workers').fill('0');
  await app.getByTestId('settings-save').click();
  await expect(app.getByTestId('toast-error')).toContainText('decode_workers');
  await expect(app.getByTestId('settings-dialog')).toBeVisible();
});
