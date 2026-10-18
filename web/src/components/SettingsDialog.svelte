<script lang="ts">
  import { untrack } from 'svelte';
  import { store } from '../lib/store.svelte';
  import type { Settings } from '../lib/types';
  import Modal from './Modal.svelte';

  let { onClose }: { onClose: () => void } = $props();

  const fallback: Settings = {
    cache_max_memory_mb: 2048,
    preview_height: 540,
    decode_workers: 2,
    hwaccel: false,
    jpeg_quality: 85,
    ffmpeg_path: null,
    ffprobe_path: null,
  };
  let s = $state<Settings>(untrack(() => ({ ...(store.settings ?? fallback) })));
  const HEIGHTS = [240, 360, 480, 540, 720, 1080, 1440, 2160];

  /** Preview frame size for the estimate (current video, else 960×540). */
  const frameSize = $derived.by(() => {
    const v = store.video?.info;
    const h =
      s.preview_height === 0 || !v ? (v?.height ?? 540) : Math.min(s.preview_height, v.height);
    const w = v ? Math.round((v.width * h) / v.height) : Math.round((h * 16) / 9);
    return { w, h };
  });
  const fits = $derived(
    Math.max(
      3,
      Math.floor((s.cache_max_memory_mb * 1024 * 1024) / (frameSize.w * frameSize.h * 3)),
    ),
  );
  const fitsSeconds = $derived(fits / store.fps);

  async function save() {
    if (await store.saveSettings($state.snapshot(s))) onClose();
  }
</script>

<Modal title="Settings" testid="settings-dialog" {onClose}>
  <div class="grid">
    <label for="mem">Preview memory</label>
    <div>
      <input
        id="mem"
        type="number"
        min="64"
        step="64"
        bind:value={s.cache_max_memory_mb}
        data-testid="settings-memory"
      />
      MiB
      <p class="muted hint" data-testid="settings-memory-estimate">
        ≈ {fits} frames at {frameSize.w}×{frameSize.h}
        ({fitsSeconds.toFixed(1)} s{store.video && fits >= store.frameCount
          ? ' — the whole video fits'
          : ''})
      </p>
    </div>

    <label for="ph">Preview resolution</label>
    <select id="ph" bind:value={s.preview_height} data-testid="settings-preview-height">
      {#each HEIGHTS as h (h)}
        <option value={h}>{h}p</option>
      {/each}
      <option value={0}>Native</option>
    </select>

    <label for="workers">Decoder processes</label>
    <input
      id="workers"
      type="number"
      min="1"
      max="16"
      bind:value={s.decode_workers}
      data-testid="settings-workers"
    />

    <label for="hw">Hardware decoding</label>
    <label class="check">
      <input id="hw" type="checkbox" bind:checked={s.hwaccel} data-testid="settings-hwaccel" />
      use <code>-hwaccel auto</code> for the preview
    </label>

    <label for="jpeg">Remote JPEG quality</label>
    <input
      id="jpeg"
      type="number"
      min="1"
      max="100"
      bind:value={s.jpeg_quality}
      data-testid="settings-jpeg"
    />

    <label for="audio">Audio</label>
    <label class="check">
      <input
        id="audio"
        type="checkbox"
        checked={store.audioEnabled}
        data-testid="settings-audio"
        onchange={(e) => store.setAudioEnabled(e.currentTarget.checked)}
      />
      play the video's audio (this browser, applies at once)
    </label>

    <label for="ffmpeg">ffmpeg</label>
    <input
      id="ffmpeg"
      type="text"
      placeholder="auto"
      value={s.ffmpeg_path ?? ''}
      data-testid="settings-ffmpeg"
      onchange={(e) => (s.ffmpeg_path = e.currentTarget.value.trim() || null)}
    />

    <label for="ffprobe">ffprobe</label>
    <input
      id="ffprobe"
      type="text"
      placeholder="auto"
      value={s.ffprobe_path ?? ''}
      data-testid="settings-ffprobe"
      onchange={(e) => (s.ffprobe_path = e.currentTarget.value.trim() || null)}
    />
  </div>
  <p class="muted hint">
    Custom ffmpeg paths apply after a restart. {store.info?.ffmpeg.version ?? ''}
  </p>

  {#snippet footer()}
    <button onclick={onClose}>Cancel</button>
    <button class="primary" data-testid="settings-save" onclick={save}>Save</button>
  {/snippet}
</Modal>

<style>
  .grid {
    display: grid;
    grid-template-columns: 150px 1fr;
    gap: 10px;
    align-items: center;
  }

  .hint {
    margin: 4px 0 0;
    font-size: 11px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  input[type='text'] {
    width: 100%;
  }
</style>
