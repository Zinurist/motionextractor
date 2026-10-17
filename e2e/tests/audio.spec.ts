import { join } from 'node:path';
import type { Page } from '@playwright/test';
import { expect, makeSquareVideo, openVideo, test } from './fixtures';

type AudioState = {
  src: string;
  paused: boolean;
  muted: boolean;
  time: number;
  level: number;
} | null;

const audioState = (app: Page) =>
  app.evaluate(
    () =>
      (window.mxDebug!.store as unknown as { debugAudio(): AudioState }).debugAudio() as AudioState,
  );

test('plays the audio along with the video', async ({ app }) => {
  // The fixture video has a sine tone.
  await expect(app.getByTestId('mute')).toBeVisible();
  expect((await audioState(app))!.src).toContain('/api/audio');
  await expect(app.getByTestId('cache-info')).toHaveAttribute('data-cached', '30');

  await app.getByTestId('play').click();
  await expect.poll(async () => (await audioState(app))!.paused).toBe(false);
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'false', {
    timeout: 10_000,
  });
  const after = (await audioState(app))!;
  expect(after.paused).toBe(true);
  // The audio ran (approximately) the length of the video (1 s).
  expect(after.time).toBeGreaterThan(0.5);
});

test('mute is remembered', async ({ app }) => {
  await app.getByTestId('mute').click();
  await expect(app.getByTestId('mute')).toHaveAttribute('data-muted', 'true');
  expect((await audioState(app))!.muted).toBe(true);
  await app.reload();
  await expect(app.getByTestId('mute')).toHaveAttribute('data-muted', 'true');
  expect((await audioState(app))!.muted).toBe(true);
  await app.getByTestId('mute').click();
  expect((await audioState(app))!.muted).toBe(false);
});

test('videos without audio have no mute button', async ({ app, server }) => {
  const silent = makeSquareVideo(join(server.dir, 'silent.mkv'), { frames: 20 });
  await openVideo(app, server, silent, 20);
  await expect(app.getByTestId('mute')).toBeHidden();
  expect(await audioState(app)).toBeNull();
});

test('volume from 0 to 200 % (boost), remembered', async ({ app }) => {
  const volume = app.getByTestId('volume');
  await expect(app.getByTestId('volume-value')).toHaveText('100%');
  await volume.fill('150');
  await expect(app.getByTestId('volume-value')).toHaveText('150%');

  // Above 100 % the audio runs through a gain node, set up on the first play.
  await expect(app.getByTestId('cache-info')).toHaveAttribute('data-cached', '30');
  await app.getByTestId('play').click();
  await expect.poll(async () => (await audioState(app))!.level).toBe(1.5);
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'false', {
    timeout: 10_000,
  });

  await volume.fill('40');
  await expect.poll(async () => (await audioState(app))!.level).toBeCloseTo(0.4);

  await app.reload();
  await expect(app.getByTestId('volume-value')).toHaveText('40%');
  // Double-click resets to 100 %.
  await app.getByTestId('volume').dblclick();
  await expect(app.getByTestId('volume-value')).toHaveText('100%');
});
