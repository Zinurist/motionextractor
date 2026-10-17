import { chmodSync, mkdtempSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  expect,
  expectFrame,
  makeSquareVideo,
  removeDir,
  startServer,
  stopServer,
  test,
} from './fixtures';

const SLOW_FFMPEG = join(dirname(fileURLToPath(import.meta.url)), 'support', 'slow-ffmpeg.mjs');

test('scrubs with the timeline', async ({ app }) => {
  const track = app.getByTestId('timeline');
  const box = (await track.boundingBox())!;
  // Frame i covers [i, i+1) / 30 of the track.
  await app.mouse.click(box.x + box.width * (10.5 / 30), box.y + box.height / 2);
  await expectFrame(app, 10);
  await expect(app.getByTestId('frame-input')).toHaveValue('10');
  await expect(app.getByTestId('time')).toContainText('00:00.33 / 00:01.00');

  // Dragging scrubs continuously.
  await app.mouse.move(box.x + box.width * (2.5 / 30), box.y + box.height / 2);
  await app.mouse.down();
  await app.mouse.move(box.x + box.width * (20.5 / 30), box.y + box.height / 2, { steps: 8 });
  await app.mouse.up();
  await expectFrame(app, 20);
});

test('steps with buttons, keyboard and the frame field', async ({ app }) => {
  await app.getByTestId('step-forward').click();
  await expectFrame(app, 1);
  await app.getByTestId('step-back').click();
  await expectFrame(app, 0);
  await app.getByTestId('go-end').click();
  await expectFrame(app, 29);
  await app.getByTestId('go-start').click();
  await expectFrame(app, 0);

  await app.keyboard.press('ArrowRight');
  await app.keyboard.press('ArrowRight');
  await expectFrame(app, 2);
  await app.keyboard.press('Shift+ArrowRight'); // one second = 30 frames, clamped
  await expectFrame(app, 29);
  await app.keyboard.press('ArrowLeft');
  await expectFrame(app, 28);
  await app.keyboard.press('Home');
  await expectFrame(app, 0);
  await app.keyboard.press('End');
  await expectFrame(app, 29);

  await app.getByTestId('frame-input').fill('17');
  await app.getByTestId('frame-input').press('Enter');
  await expectFrame(app, 17);
});

test('plays and pauses', async ({ app }) => {
  await app.getByTestId('play').click();
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'true');
  // One second of video: playback ends on the last frame.
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'false', {
    timeout: 10_000,
  });
  await expectFrame(app, 29);

  // Space restarts from the beginning; pause stops somewhere in between.
  await app.keyboard.press('Space');
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'true');
  await app.keyboard.press('Space');
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'false');
});

test('fills the preview cache in the background', async ({ app }) => {
  await expect(app.getByTestId('cache-info')).toHaveAttribute('data-cached', '30');
  await expect(app.getByTestId('cache-info')).toContainText('30 / 30 frames cached');
});

test('does not flash the loading badge while playing', async ({ app }) => {
  // With every frame cached, playback never stalls: the badge must not appear at all.
  await expect(app.getByTestId('cache-info')).toHaveAttribute('data-cached', '30');
  await app.evaluate(() => {
    const w = window as unknown as { badgeShown: number };
    w.badgeShown = 0;
    new MutationObserver(() => {
      if (document.querySelector('[data-testid="viewer-loading"]')) w.badgeShown++;
    }).observe(document.body, { subtree: true, childList: true });
  });
  await app.getByTestId('play').click();
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'false', {
    timeout: 10_000,
  });
  await expectFrame(app, 29);
  expect(await app.evaluate(() => (window as unknown as { badgeShown: number }).badgeShown)).toBe(
    0,
  );
});

test('waits for decoding instead of running ahead of the frames', async ({ browser }) => {
  test.skip(process.platform === 'win32', 'the throttling ffmpeg wrapper is a shell script');
  // Decoders deliver ~10 frames/s (160×120 RGB = 57.6 kB per frame), a third of the video's rate.
  const dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'mx-e2e-slow-')));
  const shim = join(dir, 'slow-ffmpeg');
  writeFileSync(shim, `#!/bin/sh\nexec node ${JSON.stringify(SLOW_FFMPEG)} "$@"\n`);
  chmodSync(shim, 0o755);
  const video = makeSquareVideo(join(dir, 'square.mkv'), { frames: 90 });
  const server = await startServer(dir, video, {
    MOTION_EXTRACTOR_FFMPEG: shim,
    MX_REAL_FFMPEG: process.env.MOTION_EXTRACTOR_FFMPEG ?? 'ffmpeg',
    MX_SLOW_BYTES_PER_SECOND: String(57_600 * 10),
  });
  try {
    await server.api('PUT', '/api/settings', { decode_workers: 1 });
    await server.api('POST', '/api/open', { path: video });
    const page = await browser.newPage();
    await page.goto(server.url + '/');
    await expectFrame(page, 0);

    // Sample the playhead against the shown frame on every animation frame while playing.
    await page.evaluate(() => {
      const w = window as unknown as { maxLag: number; buffered: boolean };
      const s = window.mxDebug!.store as unknown as {
        frame: number;
        playing: boolean;
        buffering: boolean;
        shown: { output: { index: number } | null };
      };
      w.maxLag = 0;
      w.buffered = false;
      const sample = () => {
        if (s.playing && !s.buffering && s.shown.output) {
          w.maxLag = Math.max(w.maxLag, s.frame - s.shown.output.index);
        }
        if (s.buffering) w.buffered = true;
        requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    });
    await page.getByTestId('play').click();
    await expect(page.getByTestId('viewer-buffering')).toBeVisible();
    await expect(page.getByTestId('play')).toHaveAttribute('data-playing', 'false', {
      timeout: 40_000,
    });
    await expectFrame(page, 89);
    const { maxLag, buffered } = await page.evaluate(() => {
      const w = window as unknown as { maxLag: number; buffered: boolean };
      return { maxLag: w.maxLag, buffered: w.buffered };
    });
    expect(buffered).toBe(true);
    // The image never falls more than a few frames behind the playhead.
    expect(maxLag).toBeLessThanOrEqual(4);
  } finally {
    await stopServer(server);
    removeDir(dir);
  }
});
