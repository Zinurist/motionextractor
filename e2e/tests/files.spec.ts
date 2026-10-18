import { mkdtempSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, expectFrame, makeSquareVideo, removeDir, test } from './fixtures';

test('uploads a video and opens it', async ({ app, server }) => {
  const elsewhere = mkdtempSync(join(tmpdir(), 'mx-e2e-upload-'));
  try {
    const local = makeSquareVideo(join(elsewhere, 'holiday.mkv'), { frames: 40 });
    await app.getByTestId('open-video').click();
    await expect(app.getByTestId('fb-upload')).toBeEnabled();
    await app.getByTestId('fb-upload-input').setInputFiles(local);
    await expect(app.getByTestId('file-browser')).toBeHidden();
    await expect(app.getByTestId('video-name')).toContainText('holiday.mkv');
    await expect(app.getByTestId('video-name')).toContainText('40 frames');
    await expectFrame(app, 0);
    expect(statSync(join(server.dir, 'holiday.mkv')).size).toBe(statSync(local).size);

    // A second upload with the same name does not overwrite the first one.
    await app.getByTestId('open-video').click();
    await expect(app.getByTestId('fb-upload')).toBeEnabled();
    await app.getByTestId('fb-upload-input').setInputFiles(local);
    await expect(app.getByTestId('video-name')).toContainText('holiday (2).mkv');
  } finally {
    removeDir(elsewhere);
  }
});

test('uploads several videos at once, of any type', async ({ app, server }) => {
  const elsewhere = mkdtempSync(join(tmpdir(), 'mx-e2e-upload-'));
  try {
    // Upper-case extensions like the .MOV files from phones must not be filtered out.
    const a = makeSquareVideo(join(elsewhere, 'IMG_0001.MOV'), { frames: 20 });
    const b = makeSquareVideo(join(elsewhere, 'IMG_0002.MOV'), { frames: 30 });
    await app.getByTestId('open-video').click();
    await expect(app.getByTestId('fb-upload-input')).not.toHaveAttribute('accept');
    await expect(app.getByTestId('fb-upload')).toBeEnabled();
    await app.getByTestId('fb-upload-input').setInputFiles([a, b]);
    // Several uploads keep the browser open with the new files listed.
    await expect(app.locator('[data-name="IMG_0002.MOV"]')).toBeVisible();
    await expect(app.locator('[data-name="IMG_0001.MOV"]')).toBeVisible();
    expect(statSync(join(server.dir, 'IMG_0001.MOV')).size).toBe(statSync(a).size);
    await app.locator('[data-name="IMG_0002.MOV"]').click();
    await expect(app.getByTestId('video-name')).toContainText('30 frames');
  } finally {
    removeDir(elsewhere);
  }
});

test('shows the free space of the directory', async ({ app }) => {
  await app.getByTestId('open-video').click();
  await expect(app.getByTestId('fb-free')).toHaveText(/^\d+(\.\d)? (B|KB|MB|GB|TB|PB) free$/);
});

test('downloads files from the file browser', async ({ app, server }) => {
  await app.getByTestId('open-video').click();
  const entry = app.locator('[data-testid="fb-entries"] li', {
    has: app.locator('[data-name="square.mkv"]'),
  });
  const [download] = await Promise.all([
    app.waitForEvent('download'),
    entry.getByTestId('fb-download').click(),
  ]);
  expect(download.suggestedFilename()).toBe('square.mkv');
  const saved = await download.path();
  expect(readFileSync(saved)).toEqual(readFileSync(server.video));
  // Directories have no download button.
  const clips = app.locator('[data-testid="fb-entries"] li', {
    has: app.locator('[data-name="clips"]'),
  });
  await expect(clips.getByTestId('fb-download')).toHaveCount(0);
});

test('downloads a finished render', async ({ app }) => {
  await app.getByTestId('open-render').click();
  await expect(app.getByTestId('render-download')).toHaveCount(0);
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'finished');
  const [download] = await Promise.all([
    app.waitForEvent('download'),
    app.getByTestId('render-download').click(),
  ]);
  expect(download.suggestedFilename()).toBe('square_motion.mp4');
  expect(statSync(await download.path()).size).toBeGreaterThan(1000);
});
