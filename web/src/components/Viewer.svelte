<script lang="ts">
  import { onMount } from 'svelte';
  import { store } from '../lib/store.svelte';
  import { createRenderer, type Quad, type Rect, type Renderer } from '../lib/renderer';
  import { clamp } from '../lib/time';

  let container: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let renderer: Renderer | null = null;
  let size = $state({ w: 0, h: 0 });
  /** Image pixels → CSS pixels; null = fit to the view. */
  let zoom = $state<number | null>(null);
  let pan = $state({ x: 0, y: 0 });
  let split = $state(0.5);
  let dragging: 'pan' | 'split' | null = null;
  let last = { x: 0, y: 0 };

  const imageSize = $derived(
    store.video ? { w: store.video.preview_width, h: store.video.preview_height } : { w: 16, h: 9 },
  );

  /** Slots (CSS pixels) the image(s) are fitted into. */
  const slots = $derived.by((): Rect[] => {
    const { w, h } = size;
    return store.viewMode === 'side'
      ? [
          { x: 0, y: 0, w: w / 2, h },
          { x: w / 2, y: 0, w: w / 2, h },
        ]
      : [{ x: 0, y: 0, w, h }];
  });

  function fitScale(slot: Rect): number {
    return Math.min(slot.w / imageSize.w, slot.h / imageSize.h);
  }

  const scale = $derived(zoom ?? (slots.length ? fitScale(slots[0]) : 1));

  /** Where the image is drawn inside a slot (CSS pixels). */
  function placement(slot: Rect): Rect {
    const w = imageSize.w * scale;
    const h = imageSize.h * scale;
    return {
      x: slot.x + (slot.w - w) / 2 + pan.x,
      y: slot.y + (slot.h - h) / 2 + pan.y,
      w,
      h,
    };
  }

  const rects = $derived(slots.map(placement));

  function draw() {
    if (!renderer || !store.video) return;
    const dpr = window.devicePixelRatio || 1;
    const px = (r: Rect): Rect => ({ x: r.x * dpr, y: r.y * dpr, w: r.w * dpr, h: r.h * dpr });
    let quads: Quad[];
    switch (store.viewMode) {
      case 'side':
        quads = [
          { rect: px(rects[0]), content: 'original' },
          { rect: px(rects[1]), content: 'output' },
        ];
        break;
      case 'split':
        quads = [{ rect: px(rects[0]), content: { split } }];
        break;
      default:
        quads = [{ rect: px(rects[0]), content: store.viewMode }];
    }
    renderer.draw(quads, scale < 2);
  }

  onMount(() => {
    renderer = createRenderer(canvas);
    const unsubscribe = store.onFrame((f) => {
      void renderer?.upload(f).then(draw);
    });
    const ro = new ResizeObserver(() => {
      const r = container.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.max(1, Math.round(r.width * dpr));
      canvas.height = Math.max(1, Math.round(r.height * dpr));
      size = { w: r.width, h: r.height };
    });
    ro.observe(container);
    return () => {
      unsubscribe();
      ro.disconnect();
    };
  });

  $effect(() => {
    // Redraw on layout changes.
    void [rects, split, store.viewMode, scale];
    draw();
  });

  const videoPath = $derived(store.video?.info.path);

  $effect(() => {
    // A new video starts fitted with blank textures.
    void videoPath;
    zoom = null;
    pan = { x: 0, y: 0 };
    renderer?.clear('output');
    renderer?.clear('original');
  });

  function onWheel(e: WheelEvent) {
    if (!store.video) return;
    e.preventDefault();
    const bounds = container.getBoundingClientRect();
    const cx = e.clientX - bounds.left;
    const cy = e.clientY - bounds.top;
    const slot = slots.find((s) => cx >= s.x && cx < s.x + s.w) ?? slots[0];
    const r = placement(slot);
    // Image coordinate under the cursor stays put.
    const u = (cx - r.x) / scale;
    const v = (cy - r.y) / scale;
    const next = clamp(scale * Math.exp(-e.deltaY * 0.0015), 0.05, 32);
    zoom = next;
    const w = imageSize.w * next;
    const h = imageSize.h * next;
    pan = {
      x: cx - u * next - (slot.x + (slot.w - w) / 2),
      y: cy - v * next - (slot.y + (slot.h - h) / 2),
    };
  }

  function onPointerDown(e: PointerEvent) {
    if (!store.video || e.button !== 0) return;
    const target = e.target as HTMLElement;
    dragging = target.dataset.handle === 'split' ? 'split' : 'pan';
    last = { x: e.clientX, y: e.clientY };
    container.setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    if (dragging === 'pan') {
      pan = { x: pan.x + e.clientX - last.x, y: pan.y + e.clientY - last.y };
    } else {
      const bounds = container.getBoundingClientRect();
      const r = rects[0];
      split = clamp((e.clientX - bounds.left - r.x) / r.w, 0, 1);
    }
    last = { x: e.clientX, y: e.clientY };
  }

  function onPointerUp(e: PointerEvent) {
    dragging = null;
    if (container.hasPointerCapture(e.pointerId)) container.releasePointerCapture(e.pointerId);
  }

  function fit() {
    zoom = null;
    pan = { x: 0, y: 0 };
  }

  function actualSize() {
    zoom = 1;
    pan = { x: 0, y: 0 };
  }

  const shownView = $derived(store.viewMode === 'original' ? 'original' : 'output');
  const shown = $derived(store.shown[shownView]);
  const loading = $derived(
    !!store.video &&
      (!shown ||
        !shown.exact ||
        shown.index !== store.frame ||
        (shown.rev !== store.rev && shownView === 'output')),
  );
  const modes = [
    ['output', 'Output'],
    ['original', 'Original'],
    ['split', 'Split'],
    ['side', 'Side by side'],
  ] as const;
</script>

<div class="viewer-wrap">
  <div class="toolbar">
    <div class="segmented" role="tablist" aria-label="View mode">
      {#each modes as [mode, label] (mode)}
        <button
          role="tab"
          aria-selected={store.viewMode === mode}
          class:active={store.viewMode === mode}
          data-testid="view-{mode}"
          onclick={() => store.setViewMode(mode)}>{label}</button
        >
      {/each}
    </div>
    <span class="spacer"></span>
    {#if store.video}
      <span class="muted" data-testid="zoom-level">{Math.round(scale * 100)}%</span>
    {/if}
    <button data-testid="zoom-fit" onclick={fit}>Fit</button>
    <button data-testid="zoom-100" onclick={actualSize}>100%</button>
  </div>
  <div
    class="viewer"
    bind:this={container}
    data-testid="viewer"
    data-frame={shown?.index ?? ''}
    data-exact={shown?.exact ?? false}
    data-rev={shown?.rev ?? ''}
    data-mode={store.viewMode}
    role="img"
    aria-label="Preview"
    onwheel={onWheel}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    ondblclick={fit}
  >
    <canvas bind:this={canvas}></canvas>
    {#if !store.video}
      <div class="empty">
        <p>Open a video to start.</p>
        {#if store.info && !store.info.ffmpeg.ok}
          <p class="error" data-testid="ffmpeg-missing">
            ffmpeg was not found: {store.info.ffmpeg.error}
          </p>
        {/if}
      </div>
    {:else}
      {#if store.viewMode === 'split'}
        <div
          class="split-handle"
          data-handle="split"
          data-testid="split-handle"
          style:left="{rects[0].x + rects[0].w * split}px"
        ></div>
      {/if}
      {#if loading}
        <div class="badge" data-testid="viewer-loading">Loading…</div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .viewer-wrap {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }

  .spacer {
    flex: 1;
  }

  .segmented {
    display: flex;
  }

  .segmented button {
    border-radius: 0;
    margin-left: -1px;
  }

  .segmented button:first-child {
    border-radius: var(--radius) 0 0 var(--radius);
  }

  .segmented button:last-child {
    border-radius: 0 var(--radius) var(--radius) 0;
  }

  .segmented button.active {
    background: var(--accent-2);
    border-color: var(--accent-2);
    color: white;
    position: relative;
  }

  .viewer {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    cursor: grab;
    touch-action: none;
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }

  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    cursor: default;
  }

  .error {
    color: var(--danger);
  }

  .split-handle {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 12px;
    margin-left: -6px;
    cursor: ew-resize;
    background: linear-gradient(
      to right,
      transparent 5px,
      var(--accent) 5px,
      var(--accent) 7px,
      transparent 7px
    );
  }

  .badge {
    position: absolute;
    top: 10px;
    right: 10px;
    background: rgb(0 0 0 / 60%);
    padding: 2px 8px;
    border-radius: var(--radius);
    color: var(--muted);
    pointer-events: none;
  }
</style>
