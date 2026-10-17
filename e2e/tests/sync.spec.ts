import {
  ROW,
  currentRev,
  expect,
  expectFrame,
  expectNewerRev,
  expectPixel,
  setNumber,
  squareX,
  test,
} from './fixtures';

test('parameter changes reach every connected client', async ({ app, server }) => {
  const other = await app.context().newPage();
  await other.goto(server.url + '/');
  await expect(other.getByTestId('connection')).toHaveAttribute('data-connected', 'true');
  await expectFrame(other, 0);
  await other.keyboard.press('End');
  await expectFrame(other, 29);

  const rev = await currentRev(other);
  await setNumber(app, 'gain-value', 0.5);
  await app.getByTestId('color').fill('#00ffff');
  await expect(other.getByTestId('gain-value')).toHaveValue('0.5');
  await expect(other.getByTestId('color')).toHaveValue('#00ffff');
  await expectNewerRev(other, rev);
  // (1 − 0.02) · 0.5 = 0.49 → cyan at 125
  await expectPixel(other, squareX(28), ROW, [0, 125, 125]);
  // Each client keeps its own playhead.
  await expectFrame(app, 0);
});
