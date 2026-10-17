// Pure helpers for the playback clock and buffering decisions (see store.svelte.ts).

/** Seconds of decoded frames required before playback (re)starts. */
export const BUFFER_SECONDS = 1;
/** Frames the shown image may trail the playhead before playback pauses to buffer. */
export const MAX_LAG_FRAMES = 3;

/** Whether all frames `[from, to)` lie in one of the cached ranges (sorted, half-open). */
export function isCached(ranges: [number, number][], from: number, to: number): boolean {
  if (to <= from) return true;
  return ranges.some(([s, e]) => s <= from && to <= e);
}

/**
 * The end (exclusive) of the frames that must be cached before playing from `frame`: one second,
 * but never past the trim end, and at most half of what the cache can hold (so a small cache can
 * still satisfy it).
 */
export function bufferEnd(frame: number, trimEnd: number, fps: number, capacity: number): number {
  const wanted = Math.ceil(fps * BUFFER_SECONDS);
  const affordable = Math.max(1, Math.floor(capacity / 2));
  return Math.min(trimEnd, frame + Math.max(1, Math.min(wanted, affordable)));
}

/** Whether the shown image trails the playhead too far (playback should wait). */
export function lagging(
  playhead: number,
  shown: { index: number; exact: boolean } | null,
): boolean {
  if (!shown) return true;
  return playhead - shown.index > MAX_LAG_FRAMES || (!shown.exact && playhead !== shown.index);
}
