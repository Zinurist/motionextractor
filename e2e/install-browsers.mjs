// Installs the Playwright browsers the E2E tests will use: those in MX_BROWSERS (default: chromium,
// like playwright.config.ts). Extra arguments are passed on, e.g. `--with-deps` on Linux CI.
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

// cli.js is not in the package exports; locate it next to package.json.
const pkg = createRequire(import.meta.url).resolve('playwright/package.json');
const cli = join(dirname(pkg), 'cli.js');
const browsers = (process.env.MX_BROWSERS ?? 'chromium').split(',');
execFileSync(process.execPath, [cli, 'install', ...browsers, ...process.argv.slice(2)], {
  stdio: 'inherit',
});
