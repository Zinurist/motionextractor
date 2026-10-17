// An ffmpeg wrapper for tests: runs the real ffmpeg (MX_REAL_FFMPEG) and, for preview decoders
// (raw frames on stdout), throttles the output to MX_SLOW_BYTES_PER_SECOND.
import { spawn } from 'node:child_process';

const real = process.env.MX_REAL_FFMPEG ?? 'ffmpeg';
const rate = Number(process.env.MX_SLOW_BYTES_PER_SECOND ?? 500_000);
const args = process.argv.slice(2);
const decoder = args.includes('pipe:1');
const child = spawn(real, args, { stdio: ['inherit', 'pipe', 'inherit'] });

if (!decoder) {
  child.stdout.pipe(process.stdout);
} else {
  const start = Date.now();
  let sent = 0;
  let pending = 0;
  let ended = false;
  const finish = () => {
    if (ended && pending === 0) process.stdout.end();
  };
  child.stdout.on('data', (chunk) => {
    child.stdout.pause();
    sent += chunk.length;
    pending++;
    // Wait until this many bytes are "allowed" at the configured rate.
    const wait = Math.max(0, (sent / rate) * 1000 - (Date.now() - start));
    setTimeout(() => {
      pending--;
      if (!process.stdout.write(chunk)) process.stdout.once('drain', () => child.stdout.resume());
      else child.stdout.resume();
      finish();
    }, wait);
  });
  // Deliver everything that is still delayed before ending.
  child.stdout.on('end', () => {
    ended = true;
    finish();
  });
}
process.stdout.on('error', () => child.kill());
for (const sig of ['SIGTERM', 'SIGINT']) process.on(sig, () => child.kill(sig));
child.on('exit', (code) => {
  process.exitCode = code ?? 1;
  if (!decoder) process.exit(process.exitCode);
});
process.stdout.on('finish', () => process.exit(process.exitCode ?? 0));
