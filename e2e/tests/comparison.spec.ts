import {
  ROW,
  SQUARE,
  currentRev,
  expect,
  expectFrame,
  expectNewerRev,
  expectPixel,
  setNumber,
  squareX,
  test,
  goTo,
} from './fixtures';

const WHITE: [number, number, number] = [255, 255, 255];
const BLACK: [number, number, number] = [0, 0, 0];

test('rolling offset of one frame lights up the moving edges', async ({ app }) => {
  await goTo(app, 10);
  // Square moved from x(9) to x(10): trailing and leading 4 px strips differ, the overlap does not.
  await expectPixel(app, squareX(9), ROW, WHITE);
  await expectPixel(app, squareX(10) + SQUARE.size - 1, ROW, WHITE);
  await expectPixel(app, squareX(10) + 4, ROW, BLACK);
  await expectPixel(app, 2, 2, BLACK);
  // Frame 0 has no earlier frame: nothing lights up.
  await app.keyboard.press('Home');
  await expectFrame(app, 0);
  await expectPixel(app, squareX(0) + 2, ROW, BLACK);
});

test('offset in frames and seconds', async ({ app }) => {
  await expect(app.getByTestId('offset-info')).toHaveText('= 0.033 s');
  const rev = await currentRev(app);
  await setNumber(app, 'offset-value', 3);
  await expect(app.getByTestId('offset-info')).toHaveText('= 0.100 s');
  await app.keyboard.press('End');
  await expectNewerRev(app, rev);
  await expectFrame(app, 29);
  // Compared with frame 26: the trailing edge strip is 12 px wide now.
  await expectPixel(app, squareX(26) + 8, ROW, WHITE);

  // Switching units keeps the distance.
  await app.getByTestId('offset-unit').selectOption('seconds');
  await expect(app.getByTestId('offset-value')).toHaveValue('0.1');
  await expect(app.getByTestId('offset-info')).toHaveText('= 3 frames');

  await setNumber(app, 'offset-value', 0.5);
  await expect(app.getByTestId('offset-info')).toHaveText('= 15 frames');
  await expectPixel(app, squareX(14) + 2, ROW, WHITE);

  // Zero is rejected.
  await setNumber(app, 'offset-value', 0);
  await expect(app.getByTestId('toast-error')).toContainText('must not be zero');
});

test('negative offsets compare with later frames', async ({ app }) => {
  await setNumber(app, 'offset-value', -2);
  await goTo(app, 5);
  // The square of frame 7 lies 8 px to the right.
  await expectPixel(app, squareX(7) + SQUARE.size - 1, ROW, WHITE);
  await expectPixel(app, squareX(5), ROW, WHITE);
  // The last two frames have no later frame.
  await app.keyboard.press('End');
  await expectFrame(app, 29);
  await expectPixel(app, squareX(29) + 2, ROW, BLACK);
});

test('fixed reference frame shows changes since that frame', async ({ app }) => {
  await goTo(app, 3);
  const rev = await currentRev(app);
  await app.getByTestId('mode-fixed').check();
  await app.getByTestId('mode-fixed').blur();
  // Switching to the fixed mode uses the current frame as reference.
  await expect(app.getByTestId('reference-frame')).toHaveValue('3');
  await expect(app.getByTestId('reference-marker')).toBeVisible();
  await app.keyboard.press('End');
  await expectNewerRev(app, rev);
  await expectFrame(app, 29);
  await expectPixel(app, squareX(3) + 8, ROW, WHITE); // where it was
  await expectPixel(app, squareX(29) + 8, ROW, WHITE); // where it is
  await expectPixel(app, squareX(15), ROW, BLACK);

  // The reference itself shows no change.
  await setNumber(app, 'reference-frame', 29);
  await expectPixel(app, squareX(29) + 8, ROW, BLACK);
  // "Use current" picks the playhead.
  await app.keyboard.press('Home');
  await app.getByTestId('reference-current').click();
  await expect(app.getByTestId('reference-frame')).toHaveValue('0');

  // Back to the rolling mode with the previous offset.
  await app.getByTestId('mode-rolling').check();
  await expect(app.getByTestId('offset-value')).toHaveValue('1');
  await expect(app.getByTestId('reference-marker')).toBeHidden();
});
