import { ROW, SQUARE, expect, expectFrame, expectPixel, goTo, squareX, test } from './fixtures';

test('original, split and side-by-side views', async ({ app }) => {
  await goTo(app, 10);

  await app.getByTestId('view-original').click();
  await expect(app.getByTestId('viewer')).toHaveAttribute('data-mode', 'original');
  await expectFrame(app, 10);
  await expectPixel(app, squareX(10) + 4, ROW, [255, 255, 255], 'original');

  await app.getByTestId('view-split').click();
  await expect(app.getByTestId('split-handle')).toBeVisible();
  // Both images are kept up to date while scrubbing.
  await app.keyboard.press('ArrowRight');
  await expectFrame(app, 11);
  await expectPixel(app, squareX(11) + 4, ROW, [255, 255, 255], 'original');
  await expectPixel(app, squareX(11) + SQUARE.size - 1, ROW, [255, 255, 255], 'output');

  // The split handle can be dragged.
  const handle = app.getByTestId('split-handle');
  const before = (await handle.boundingBox())!;
  await app.mouse.move(before.x + 6, before.y + 50);
  await app.mouse.down();
  await app.mouse.move(before.x + 106, before.y + 50, { steps: 4 });
  await app.mouse.up();
  const after = (await handle.boundingBox())!;
  expect(after.x).toBeGreaterThan(before.x + 50);

  await app.getByTestId('view-side').click();
  await expect(app.getByTestId('split-handle')).toBeHidden();
  await expect(app.getByTestId('viewer')).toHaveAttribute('data-mode', 'side');
});

test('zoom and pan', async ({ app }) => {
  const zoom = app.getByTestId('zoom-level');
  const fitted = await zoom.textContent();
  await app.getByTestId('zoom-100').click();
  await expect(zoom).toHaveText('100%');
  const viewer = app.getByTestId('viewer');
  const box = (await viewer.boundingBox())!;
  await app.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await app.mouse.wheel(0, -400);
  await expect.poll(async () => parseInt((await zoom.textContent())!)).toBeGreaterThan(100);
  // Dragging pans (the zoom stays).
  const z = await zoom.textContent();
  await app.mouse.down();
  await app.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 30, { steps: 4 });
  await app.mouse.up();
  await expect(zoom).toHaveText(z!);
  await app.getByTestId('zoom-fit').click();
  await expect(zoom).toHaveText(fitted!);
  // Double click fits as well.
  await app.getByTestId('zoom-100').click();
  await viewer.dblclick();
  await expect(zoom).toHaveText(fitted!);
});
