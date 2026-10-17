import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  ROW,
  currentRev,
  expect,
  expectNewerRev,
  expectPixel,
  goTo,
  setNumber,
  squareX,
  test,
} from './fixtures';

test('saves and loads presets', async ({ app, server }) => {
  await goTo(app, 10);
  let rev = await currentRev(app);
  await app.getByTestId('color').fill('#ff0000');
  await setNumber(app, 'offset-value', 2);
  rev = await expectNewerRev(app, rev);

  await app.getByTestId('save-preset').click();
  await expect(app.getByTestId('fb-name')).toHaveValue('square_motion.json');
  await app.getByTestId('fb-confirm').click();
  await expect(app.getByTestId('toast-info')).toContainText('Preset saved');
  const file = join(server.dir, 'square_motion.json');
  expect(existsSync(file)).toBe(true);
  const saved = JSON.parse(readFileSync(file, 'utf8'));
  expect(saved.effect.color).toBe('#ff0000');
  expect(saved.comparison.offset.value).toBe(2);

  // Change things, then load the preset back.
  await app.getByTestId('color').fill('#0000ff');
  await setNumber(app, 'offset-value', 1);
  rev = await expectNewerRev(app, rev);
  await expectPixel(app, squareX(9), ROW, [0, 0, 255]);
  await app.getByTestId('load-preset').click();
  await app.locator('[data-name="square_motion.json"]').click();
  await expect(app.getByTestId('color')).toHaveValue('#ff0000');
  await expect(app.getByTestId('offset-value')).toHaveValue('2');
  await expectNewerRev(app, rev);
  await expectPixel(app, squareX(8), ROW, [255, 0, 0]);
});

test('rejects invalid preset files', async ({ app, server }) => {
  writeFileSync(join(server.dir, 'bad.json'), '{"effect": {"threshold": 9}}');
  await app.getByTestId('load-preset').click();
  await app.locator('[data-name="bad.json"]').click();
  await expect(app.getByTestId('toast-error')).toContainText('threshold');
});
