import { describe, expect, it } from 'vitest';
import { parseFrame, FRAME_MAGIC } from './protocol';
import { clamp, formatDuration, formatTime, offsetFrames } from './time';
import { basename, defaultOutput, dirname, join, withExtension } from './paths';

describe('protocol', () => {
  it('parses frame headers', () => {
    const buf = new ArrayBuffer(32 + 6);
    const v = new DataView(buf);
    [FRAME_MAGIC, 7, 42, 2, 1, 0, 3, 9].forEach((w, i) => v.setUint32(i * 4, w, true));
    new Uint8Array(buf, 32).set([1, 2, 3, 4, 5, 6]);
    const f = parseFrame(buf)!;
    expect(f).toMatchObject({ seq: 7, index: 42, width: 2, height: 1, exact: true, rev: 9 });
    expect(f.view).toBe('original');
    expect(Array.from(f.payload)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(parseFrame(new ArrayBuffer(8))).toBeNull();
    expect(parseFrame(new ArrayBuffer(32))).toBeNull();
  });
});

describe('time', () => {
  it('converts offsets like the backend', () => {
    expect(offsetFrames(5, 'frames', 30)).toBe(5);
    expect(offsetFrames(0.5, 'seconds', 30)).toBe(15);
    expect(offsetFrames(0.001, 'seconds', 30)).toBe(1);
    expect(offsetFrames(-0.001, 'seconds', 30)).toBe(-1);
    expect(offsetFrames(1, 'seconds', 29.97)).toBe(30);
  });

  it('formats times', () => {
    expect(formatTime(0)).toBe('00:00.00');
    expect(formatTime(62.5)).toBe('01:02.50');
    expect(formatTime(3725.25)).toBe('1:02:05.25');
    expect(formatDuration(75)).toBe('1m 15s');
    expect(formatDuration(null)).toBe('–');
    expect(clamp(5, 0, 3)).toBe(3);
  });
});

describe('paths', () => {
  it('handles posix and windows paths', () => {
    expect(basename('/a/b/c.mp4')).toBe('c.mp4');
    expect(basename('C:\\v\\c.mp4')).toBe('c.mp4');
    expect(dirname('/a/b/c.mp4')).toBe('/a/b');
    expect(dirname('/c.mp4')).toBe('/');
    expect(dirname('C:\\v\\c.mp4')).toBe('C:\\v');
    expect(join('/a', 'b')).toBe('/a/b');
    expect(join('C:\\v', 'b')).toBe('C:\\v\\b');
    expect(join('/', 'b')).toBe('/b');
    expect(withExtension('/a/clip.mp4', 'webm')).toBe('/a/clip.webm');
    expect(withExtension('/a/frames', '')).toBe('/a/frames');
    expect(defaultOutput('/v/clip.mov', 'mp4')).toBe('/v/clip_motion.mp4');
    expect(defaultOutput('C:\\v\\clip.mov', '')).toBe('C:\\v\\clip_motion');
  });
});

import { joinArgs, splitArgs } from './args';
import { deepMerge } from './merge';

describe('args', () => {
  it('splits with quotes', () => {
    expect(splitArgs('-tune grain')).toEqual(['-tune', 'grain']);
    expect(splitArgs(`-metadata "title=My clip" -x ''`)).toEqual([
      '-metadata',
      'title=My clip',
      '-x',
      '',
    ]);
    expect(splitArgs('  ')).toEqual([]);
    expect(splitArgs(joinArgs(['a b', 'c']))).toEqual(['a b', 'c']);
  });
});

describe('merge', () => {
  it('merges deeply and replaces arrays and nulls', () => {
    expect(deepMerge({ a: { b: 1, c: 2 }, l: [1] }, { a: { b: 3 }, l: [2] })).toEqual({
      a: { b: 3, c: 2 },
      l: [2],
    });
    expect(deepMerge({ crf: 5 }, { crf: null })).toEqual({ crf: null });
  });
});

describe('render job updates', () => {
  it('ignores updates older than the shown state', async () => {
    const { store } = await import('./store.svelte');
    const job = (id: number, state: 'running' | 'finished' | 'failed') => ({
      id,
      state,
      done: 0,
      total: 1,
      fps: 0,
      eta_seconds: null,
      elapsed_seconds: 0,
      output: 'o.mp4',
      command: '',
      error: null,
    });
    store.setJob(job(1, 'running'));
    store.setJob(job(1, 'failed'));
    // The late response to the start request must not resurrect the job.
    store.setJob(job(1, 'running'));
    expect(store.job?.state).toBe('failed');
    store.setJob(job(2, 'running'));
    expect(store.job?.id).toBe(2);
    store.setJob(job(1, 'finished'));
    expect(store.job?.id).toBe(2);
  });
});

import { bufferEnd, isCached, lagging } from './playback';

describe('playback', () => {
  it('checks cached ranges', () => {
    const ranges: [number, number][] = [
      [0, 10],
      [20, 30],
    ];
    expect(isCached(ranges, 2, 10)).toBe(true);
    expect(isCached(ranges, 8, 12)).toBe(false);
    expect(isCached(ranges, 20, 30)).toBe(true);
    expect(isCached(ranges, 15, 15)).toBe(true);
    expect(isCached([], 0, 1)).toBe(false);
  });

  it('buffers one second, bounded by the trim and the cache', () => {
    expect(bufferEnd(10, 1000, 30, 1000)).toBe(40);
    expect(bufferEnd(10, 25, 30, 1000)).toBe(25);
    expect(bufferEnd(10, 1000, 30, 20)).toBe(20);
    expect(bufferEnd(10, 1000, 30, 0)).toBe(11);
  });

  it('detects a lagging image', () => {
    expect(lagging(10, { index: 10, exact: true })).toBe(false);
    expect(lagging(13, { index: 10, exact: true })).toBe(false);
    expect(lagging(14, { index: 10, exact: true })).toBe(true);
    expect(lagging(10, { index: 8, exact: false })).toBe(true);
    expect(lagging(10, null)).toBe(true);
  });
});
