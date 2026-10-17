<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { formatTime } from '../lib/time';
  import Icon from './Icon.svelte';

  let track: HTMLDivElement;
  let scrubbing = false;
  /** Which trim edge is being dragged. */
  let dragging = $state<'start' | 'end' | null>(null);

  const count = $derived(store.frameCount);
  // Frame i occupies [i, i + 1) of the track; the playhead sits at the frame's centre.
  const pct = (i: number) => (count > 0 ? Math.min(100, (i / count) * 100) : 0);
  const centre = (i: number) => pct(i + 0.5);
  const refFrame = $derived(
    store.preset.comparison.mode === 'fixed' ? store.preset.comparison.frame : null,
  );
  const trim = $derived(store.trim);

  function frameAt(clientX: number): number {
    const r = track.getBoundingClientRect();
    return Math.floor(((clientX - r.left) / r.width) * count);
  }

  /** The frame boundary (0..count) closest to a pointer position. */
  function boundaryAt(clientX: number): number {
    const r = track.getBoundingClientRect();
    return Math.round(((clientX - r.left) / r.width) * count);
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

  function grabEdge(e: PointerEvent, edge: 'start' | 'end') {
    if (!store.video || e.button !== 0) return;
    // Dragging an edge must not scrub the track underneath.
    e.stopPropagation();
    store.pause();
    dragging = edge;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function moveEdge(e: PointerEvent) {
    if (!dragging) return;
    e.stopPropagation();
    const b = boundaryAt(e.clientX);
    if (dragging === 'start') {
      const start = Math.min(Math.max(0, b), trim.end - 1);
      store.setTrim(start, trim.end);
      // Show the frame the trimmed video starts with.
      store.setFrame(start);
    } else {
      const end = Math.max(Math.min(count, b), trim.start + 1);
      store.setTrim(trim.start, end);
      store.setFrame(end - 1);
    }
  }

  function releaseEdge(e: PointerEvent) {
    if (!dragging) return;
    e.stopPropagation();
    dragging = null;
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
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
      title="Start of the trimmed video (Home)"
      data-testid="go-start"
      onclick={() => store.goStart()}><Icon name="start" /></button
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
    <button
      class="icon"
      title="End of the trimmed video (End)"
      data-testid="go-end"
      onclick={() => store.goEnd()}><Icon name="end" /></button
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
    {#if store.trimmed}
      <span
        class="trim-info"
        data-testid="trim-info"
        title="Only this part is played and rendered (I / O set the in and out points)"
      >
        Trim {formatTime(trim.start / store.fps)} – {formatTime(trim.end / store.fps)}
        <span class="muted">({trim.end - trim.start} frames)</span>
        <button
          class="icon reset"
          aria-label="Remove trim"
          title="Remove trim"
          data-testid="trim-reset"
          onclick={() => store.setTrim(0, count)}>✕</button
        >
      </span>
    {/if}
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
    {#each store.cache.ranges as [s, e] (s)}
      <div
        class="cached"
        style:left="{pct(s)}%"
        style:width="{Math.max(0.2, pct(e) - pct(s))}%"
      ></div>
    {/each}
    {#if count}
      <!-- Everything outside the trim window is dimmed. -->
      <div class="outside" style:left="0" style:width="{pct(trim.start)}%"></div>
      <div class="outside" style:left="{pct(trim.end)}%" style:right="0"></div>
      <div
        class="trim"
        class:trimmed={store.trimmed}
        data-testid="trim-window"
        data-start={trim.start}
        data-end={trim.end}
        style:left="{pct(trim.start)}%"
        style:width="{pct(trim.end) - pct(trim.start)}%"
      >
        <div
          class="edge start"
          class:active={dragging === 'start'}
          role="slider"
          tabindex="-1"
          aria-label="Trim start"
          aria-valuenow={trim.start}
          title="Drag to trim the start"
          data-testid="trim-start"
          onpointerdown={(e) => grabEdge(e, 'start')}
          onpointermove={moveEdge}
          onpointerup={releaseEdge}
        ></div>
        <div
          class="edge end"
          class:active={dragging === 'end'}
          role="slider"
          tabindex="-1"
          aria-label="Trim end"
          aria-valuenow={trim.end}
          title="Drag to trim the end"
          data-testid="trim-end"
          onpointerdown={(e) => grabEdge(e, 'end')}
          onpointermove={moveEdge}
          onpointerup={releaseEdge}
        ></div>
      </div>
    {/if}
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

  .trim-info {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: 8px;
    color: var(--trim);
  }

  .trim-info .reset {
    min-width: 0;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--muted);
  }

  .spacer {
    flex: 1;
  }

  .track {
    position: relative;
    height: 26px;
    margin: 0 6px;
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

  .outside {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgb(0 0 0 / 55%);
    pointer-events: none;
  }

  .trim {
    position: absolute;
    top: -3px;
    bottom: -3px;
    border: 2px solid transparent;
    border-radius: var(--radius);
    pointer-events: none;
  }

  .trim.trimmed {
    border-color: var(--trim);
  }

  .edge {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: 10px;
    border-radius: 3px;
    background: var(--trim);
    opacity: 0.35;
    cursor: ew-resize;
    pointer-events: auto;
  }

  .trim.trimmed .edge,
  .edge:hover,
  .edge.active {
    opacity: 1;
  }

  .edge.start {
    left: -8px;
  }

  .edge.end {
    right: -8px;
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
