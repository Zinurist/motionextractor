<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { formatTime } from '../lib/time';
  import Icon from './Icon.svelte';

  let track: HTMLDivElement;
  let scrubbing = false;

  const count = $derived(store.frameCount);
  // Frame i occupies [i, i + 1) of the track; the playhead sits at the frame's centre.
  const pct = (i: number) => (count > 0 ? Math.min(100, (i / count) * 100) : 0);
  const centre = (i: number) => pct(i + 0.5);
  const refFrame = $derived(
    store.preset.comparison.mode === 'fixed' ? store.preset.comparison.frame : null,
  );
  const range = $derived({
    start: store.preset.render.start ?? 0,
    end: store.preset.render.end ?? count,
  });
  const hasRange = $derived(store.preset.render.start !== null || store.preset.render.end !== null);

  function frameAt(clientX: number): number {
    const r = track.getBoundingClientRect();
    return Math.floor(((clientX - r.left) / r.width) * count);
  }

  function down(e: PointerEvent) {
    if (!store.video) return;
    scrubbing = true;
    store.pause();
    track.setPointerCapture(e.pointerId);
    store.setFrame(frameAt(e.clientX));
  }

  function move(e: PointerEvent) {
    if (scrubbing) store.setFrame(frameAt(e.clientX));
  }

  function up(e: PointerEvent) {
    scrubbing = false;
    if (track.hasPointerCapture(e.pointerId)) track.releasePointerCapture(e.pointerId);
  }

  function jumpTo(raw: string) {
    const v = Number(raw);
    if (Number.isFinite(v)) {
      store.pause();
      store.setFrame(v);
    }
  }
</script>

<div class="timeline" class:disabled={!store.video}>
  <div class="controls">
    <button
      class="icon"
      title="Start (Home)"
      data-testid="go-start"
      onclick={() => store.step(-count)}><Icon name="start" /></button
    >
    <button
      class="icon"
      title="Previous frame (←)"
      data-testid="step-back"
      onclick={() => store.step(-1)}><Icon name="prev" /></button
    >
    <button
      class="icon play"
      title="Play/pause (Space)"
      data-testid="play"
      data-playing={store.playing}
      onclick={() => store.togglePlay()}><Icon name={store.playing ? 'pause' : 'play'} /></button
    >
    <button
      class="icon"
      title="Next frame (→)"
      data-testid="step-forward"
      onclick={() => store.step(1)}><Icon name="next" /></button
    >
    <button class="icon" title="End (End)" data-testid="go-end" onclick={() => store.step(count)}
      ><Icon name="end" /></button
    >
    <span class="time mono" data-testid="time">
      {formatTime(store.frame / store.fps)} / {formatTime(count / store.fps)}
    </span>
    <label class="muted frame">
      Frame
      <input
        type="number"
        min="0"
        max={Math.max(0, count - 1)}
        value={store.frame}
        data-testid="frame-input"
        onchange={(e) => jumpTo(e.currentTarget.value)}
      />
      <span>/ {Math.max(0, count - 1)}</span>
    </label>
    <span class="spacer"></span>
    <span class="muted cache-info" data-testid="cache-info" data-cached={store.cache.cached}>
      {store.cache.decoding ? 'Decoding… ' : ''}{store.cache.cached} / {count} frames cached
    </span>
  </div>
  <div
    class="track"
    bind:this={track}
    data-testid="timeline"
    role="slider"
    tabindex="-1"
    aria-label="Timeline"
    aria-valuemin={0}
    aria-valuemax={Math.max(0, count - 1)}
    aria-valuenow={store.frame}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
  >
    {#if hasRange}
      <div
        class="range"
        style:left="{pct(range.start)}%"
        style:width="{pct(range.end) - pct(range.start)}%"
      ></div>
    {/if}
    {#each store.cache.ranges as [s, e] (s)}
      <div
        class="cached"
        style:left="{pct(s)}%"
        style:width="{Math.max(0.2, pct(e) - pct(s))}%"
      ></div>
    {/each}
    {#if refFrame !== null && count}
      <div
        class="ref"
        data-testid="reference-marker"
        style:left="{centre(refFrame)}%"
        title="Reference frame {refFrame}"
      ></div>
    {/if}
    {#if count}
      <div class="playhead" style:left="{centre(store.frame)}%"></div>
    {/if}
  </div>
</div>

<style>
  .timeline {
    border-top: 1px solid var(--border);
    background: var(--panel);
    padding: 6px 10px 10px;
  }

  .timeline.disabled {
    opacity: 0.6;
    pointer-events: none;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }

  .play {
    min-width: 40px;
  }

  .time {
    margin-left: 8px;
  }

  .frame {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-left: 8px;
  }

  .spacer {
    flex: 1;
  }

  .track {
    position: relative;
    height: 26px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    touch-action: none;
  }

  .cached {
    position: absolute;
    bottom: 0;
    height: 5px;
    background: var(--cached);
  }

  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgb(79 157 255 / 14%);
  }

  .ref {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: #f5b14c;
  }

  .playhead {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    pointer-events: none;
  }
</style>
