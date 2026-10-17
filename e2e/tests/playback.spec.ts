import { expect, expectFrame, test } from './fixtures';

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
