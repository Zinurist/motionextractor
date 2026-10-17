import { join } from 'node:path';
import type { Page } from '@playwright/test';
import {
  expect,
  expectFrame,
  goTo,
  makeSquareVideo,
  openVideo,
  probeFrames,
  test,
} from './fixtures';

/** Drags a trim edge to frame boundary `b` (0..30 for the 30-frame fixture). */
async function dragEdge(app: Page, edge: 'start' | 'end', b: number) {
  const track = (await app.getByTestId('timeline').boundingBox())!;
  const handle = (await app.getByTestId(`trim-${edge}`).boundingBox())!;
  await app.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
  await app.mouse.down();
  await app.mouse.move(track.x + (track.width * b) / 30, handle.y + handle.height / 2, {
    steps: 6,
  });
  await app.mouse.up();
}

async function expectTrim(app: Page, start: number, end: number) {
  const win = app.getByTestId('trim-window');
  await expect(win).toHaveAttribute('data-start', String(start));
  await expect(win).toHaveAttribute('data-end', String(end));
}

test('trims by dragging the window edges', async ({ app, server }) => {
  await expectTrim(app, 0, 30);
  await expect(app.getByTestId('trim-info')).toBeHidden();

  await dragEdge(app, 'start', 5);
  await expectTrim(app, 5, 30);
  // While dragging, the playhead shows the first frame of the trimmed video.
  await expectFrame(app, 5);
  await dragEdge(app, 'end', 20);
  await expectTrim(app, 5, 20);
  await expectFrame(app, 19);
  await expect(app.getByTestId('trim-info')).toContainText('(15 frames)');

  // The trim is the render range (and part of the session).
  const { preset } = await server.api<{ preset: { render: { start: number; end: number } } }>(
    'GET',
    '/api/preset',
  );
  expect(preset.render).toMatchObject({ start: 5, end: 20 });

  // An edge cannot pass the other one.
  await dragEdge(app, 'start', 28);
  await expectTrim(app, 19, 20);
});

test('playback and jumps stay inside the trim', async ({ app }) => {
  await dragEdge(app, 'start', 5);
  await dragEdge(app, 'end', 20);
  await app.keyboard.press('Home');
  await expectFrame(app, 5);
  await app.keyboard.press('End');
  await expectFrame(app, 19);
  await app.getByTestId('go-start').click();
  await expectFrame(app, 5);

  // Playing from the end of the trim restarts at its start and stops at its end.
  await app.getByTestId('go-end').click();
  await app.getByTestId('play').click();
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'false', {
    timeout: 10_000,
  });
  await expectFrame(app, 19);

  // Scrubbing outside the trim is still possible; playing from there starts at the trim start.
  await goTo(app, 25);
  await app.keyboard.press('Space');
  await expect(app.getByTestId('play')).toHaveAttribute('data-playing', 'false', {
    timeout: 10_000,
  });
  await expectFrame(app, 19);
});

test('I and O set the trim at the playhead; reset removes it', async ({ app }) => {
  await goTo(app, 10);
  await app.keyboard.press('i');
  await expectTrim(app, 10, 30);
  await goTo(app, 12);
  await app.keyboard.press('o');
  await expectTrim(app, 10, 13);
  // An in point after the out point moves the out point to the end.
  await goTo(app, 20);
  await app.keyboard.press('i');
  await expectTrim(app, 20, 30);

  await app.getByTestId('trim-reset').click();
  await expectTrim(app, 0, 30);
  await expect(app.getByTestId('trim-info')).toBeHidden();
});

test('renders only the trimmed part', async ({ app, server }) => {
  await dragEdge(app, 'start', 5);
  await dragEdge(app, 'end', 20);
  await app.getByTestId('open-render').click();
  await expect(app.getByTestId('render-start')).toHaveValue('5');
  await expect(app.getByTestId('render-end')).toHaveValue('20');
  await app.getByTestId('render-start-button').click();
  await expect(app.getByTestId('render-job')).toHaveAttribute('data-state', 'finished');
  expect(probeFrames(join(server.dir, 'square_motion.mp4'))).toBe(15);
});

test('opening another video clears the trim', async ({ app, server }) => {
  await dragEdge(app, 'start', 5);
  await expectTrim(app, 5, 30);
  const other = makeSquareVideo(join(server.dir, 'other.mkv'), { frames: 45 });
  await openVideo(app, server, other, 45);
  await expectTrim(app, 0, 45);
  await expect(app.getByTestId('trim-info')).toBeHidden();
});
