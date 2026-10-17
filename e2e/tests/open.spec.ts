import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, expectFrame, makeSquareVideo, test } from './fixtures';

test('opens a video through the file browser', async ({ page, server }) => {
  await page.goto(server.url + '/');
  await expect(page.getByText('Open a video to start.')).toBeVisible();
  await expect(page.getByTestId('open-render')).toBeDisabled();

  await page.getByTestId('open-video').click();
  const browser = page.getByTestId('file-browser');
  await expect(browser).toBeVisible();
  // Directories first, hidden files skipped.
  await expect(browser.getByTestId('fb-entry').first()).toHaveAttribute('data-name', 'clips');
  await browser.locator('[data-name="square.mkv"]').click();
  await expect(browser).toBeHidden();

  await expect(page.getByTestId('video-name')).toContainText('square.mkv');
  await expect(page.getByTestId('video-name')).toContainText('160×120');
  await expect(page.getByTestId('video-name')).toContainText('30 frames');
  await expectFrame(page, 0);
  await expect(page.getByTestId('open-render')).toBeEnabled();
});

test('navigates directories and opens another video', async ({ app, server }) => {
  makeSquareVideo(join(server.dir, 'clips', 'long.mkv'), { frames: 45 });
  await app.getByTestId('open-video').click();
  const browser = app.getByTestId('file-browser');
  await browser.locator('[data-name="clips"]').click();
  await expect(browser.getByTestId('fb-path')).toHaveText(join(server.dir, 'clips'));
  await browser.locator('[data-name="long.mkv"]').click();
  await expect(app.getByTestId('video-name')).toContainText('45 frames');
  await expectFrame(app, 0);
  // Back up again.
  await app.getByTestId('open-video').click();
  await browser.getByTestId('fb-up').click();
  await expect(browser.getByTestId('fb-path')).toHaveText(server.dir);
  await browser.getByRole('button', { name: 'Cancel' }).click();
  await expect(browser).toBeHidden();
});

test('reports files that are not videos', async ({ app, server }) => {
  writeFileSync(join(server.dir, 'notes.txt'), 'hello');
  await app.getByTestId('open-video').click();
  await app.locator('[data-name="notes.txt"]').click();
  await expect(app.getByTestId('toast-error')).toContainText('ffprobe');
  // The previous video stays open.
  await expect(app.getByTestId('video-name')).toContainText('square.mkv');
});

test('shows the connection state for every client', async ({ app, server }) => {
  await expect(app.getByTestId('connection')).toHaveAttribute('data-connected', 'true');
  // A fresh tab of the same server connects as well (multiple clients).
  const other = await app.context().newPage();
  await other.goto(server.url + '/');
  await expect(other.getByTestId('connection')).toHaveAttribute('data-connected', 'true');
  server.proc.kill();
  await expect(app.getByTestId('connection')).toHaveAttribute('data-connected', 'false');
});
