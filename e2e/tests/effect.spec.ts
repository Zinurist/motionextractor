import type { Page } from '@playwright/test';
import {
  ROW,
  SQUARE,
  currentRev,
  expect,
  expectFrame,
  expectNewerRev,
  expectPixel,
  sample,
  setNumber,
  squareX,
  test,
  goTo,
} from './fixtures';

/** Frame 10: x(9)..x(9)+3 is a lit trailing edge, x(10)+4 lies in the (dark) overlap. */
const EDGE = squareX(9) + 1;
const OVERLAP = squareX(10) + 4;

async function atFrame10(app: Page) {
  await goTo(app, 10);
}

test('threshold, gain and gamma shape the motion mask', async ({ app }) => {
  await atFrame10(app);
  await expectPixel(app, EDGE, ROW, [255, 255, 255]);
  let rev = await currentRev(app);
  await setNumber(app, 'gain-value', 0.5);
  rev = await expectNewerRev(app, rev);
  // (1 − 0.02) · 0.5 = 0.49 → 125
  await expectPixel(app, EDGE, ROW, [125, 125, 125]);
  await setNumber(app, 'gamma-value', 2);
  rev = await expectNewerRev(app, rev);
  // 0.49^(1/2) = 0.7 → 179
  await expectPixel(app, EDGE, ROW, [179, 179, 179]);
  await setNumber(app, 'threshold-value', 0.5);
  await expectNewerRev(app, rev);
  // (1 − 0.5) · 0.5 = 0.25 → √ = 0.5 → 128
  await expectPixel(app, EDGE, ROW, [128, 128, 128]);
  // The slider drives the same value.
  await expect(app.getByTestId('threshold')).toHaveValue('0.5');
});

test('motion color comes from the color picker', async ({ app }) => {
  await atFrame10(app);
  const rev = await currentRev(app);
  await app.getByTestId('color').fill('#ff8000');
  await expectNewerRev(app, rev);
  await expectPixel(app, EDGE, ROW, [255, 128, 0]);
  await expectPixel(app, OVERLAP, ROW, [0, 0, 0]);
});

test('bloom adds a glow around motion', async ({ app }) => {
  await atFrame10(app);
  const below = SQUARE.y + SQUARE.size + 3;
  await expectPixel(app, EDGE, below, [0, 0, 0]);
  const rev = await currentRev(app);
  await app.getByTestId('bloom-enabled').check();
  await expectNewerRev(app, rev);
  await expect.poll(async () => (await sample(app, EDGE, below))![0]).toBeGreaterThan(20);
  const glow = (await sample(app, EDGE, below))![0];
  // A larger radius spreads it further.
  await setNumber(app, 'bloom-radius-value', 0.1);
  await expect.poll(async () => (await sample(app, EDGE, below + 20))![0]).toBeGreaterThan(5);
  await setNumber(app, 'bloom-intensity-value', 0);
  await expectPixel(app, EDGE, below, [0, 0, 0]);
  expect(glow).toBeGreaterThan(20);
});

test('overlay draws the motion on top of the original video', async ({ app }) => {
  await atFrame10(app);
  let rev = await currentRev(app);
  await expect(app.getByTestId('background-brightness')).toBeDisabled();
  await app.getByTestId('background-original').check();
  rev = await expectNewerRev(app, rev);
  // The square (present in both frames) now shows the original white instead of black.
  await expectPixel(app, OVERLAP, ROW, [255, 255, 255]);
  await app.getByTestId('color').fill('#00ff00');
  rev = await expectNewerRev(app, rev);
  await expectPixel(app, EDGE, ROW, [0, 255, 0]);
  await setNumber(app, 'background-brightness-value', 0.5);
  rev = await expectNewerRev(app, rev);
  await expectPixel(app, OVERLAP, ROW, [128, 128, 128]);
  await setNumber(app, 'overlay-opacity-value', 0.5);
  await expectNewerRev(app, rev);
  await expectPixel(app, EDGE, ROW, [0, 128, 0]);
  await app.getByTestId('background-black').check();
  await expectPixel(app, OVERLAP, ROW, [0, 0, 0]);
});

test('classic mode averages the frame with the inverted reference', async ({ app }) => {
  await atFrame10(app);
  const rev = await currentRev(app);
  await app.getByTestId('blend').selectOption('classic');
  await expectNewerRev(app, rev);
  await expectPixel(app, 2, 2, [128, 128, 128]); // static: grey
  await expectPixel(app, squareX(10) + SQUARE.size - 1, ROW, [255, 255, 255]); // appeared
  await expectPixel(app, squareX(9), ROW, [0, 0, 0]); // disappeared
  await expect(app.getByTestId('gain')).toBeDisabled();
});

test('metric and denoise are applied', async ({ app, server }) => {
  await atFrame10(app);
  let rev = await currentRev(app);
  await app.getByTestId('metric').selectOption('max');
  rev = await expectNewerRev(app, rev);
  await setNumber(app, 'denoise-value', 0.01);
  await expectNewerRev(app, rev);
  // Denoise blurs the inputs: the hard edge becomes soft.
  await expect.poll(async () => (await sample(app, EDGE, SQUARE.y - 1))![0]).toBeGreaterThan(0);
  const { preset } = await server.api<{
    preset: { effect: { metric: string; denoise: number } };
  }>('GET', '/api/preset');
  expect(preset.effect.metric).toBe('max');
  expect(preset.effect.denoise).toBe(0.01);
});

test('modified sliders can be reset to their defaults', async ({ app }) => {
  await atFrame10(app);
  // Fresh session: every slider is at its default (also proves UI and backend defaults agree).
  await expect(app.getByTestId('params').locator('[data-testid$="-reset"]')).toHaveCount(0);

  let rev = await currentRev(app);
  await setNumber(app, 'gain-value', 0.5);
  rev = await expectNewerRev(app, rev);
  await expectPixel(app, EDGE, ROW, [125, 125, 125]);
  await expect(app.getByTestId('gain-reset')).toBeVisible();
  await expect(app.getByTestId('threshold-reset')).toBeHidden();

  await app.getByTestId('gain-reset').click();
  await expect(app.getByTestId('gain-value')).toHaveValue('4');
  await expect(app.getByTestId('gain-reset')).toBeHidden();
  await expectNewerRev(app, rev);
  await expectPixel(app, EDGE, ROW, [255, 255, 255]);

  // Nested values (bloom) reset too.
  await app.getByTestId('bloom-enabled').check();
  await setNumber(app, 'bloom-radius-value', 0.1);
  await app.getByTestId('bloom-radius-reset').click();
  await expect(app.getByTestId('bloom-radius-value')).toHaveValue('0.02');
});
