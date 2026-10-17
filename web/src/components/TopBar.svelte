<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { basename } from '../lib/paths';

  let {
    onOpen,
    onRender,
    onSettings,
    onSavePreset,
    onLoadPreset,
  }: {
    onOpen: () => void;
    onRender: () => void;
    onSettings: () => void;
    onSavePreset: () => void;
    onLoadPreset: () => void;
  } = $props();

  const job = $derived(store.job);
</script>

<header class="topbar">
  <span class="logo">Motion Extractor</span>
  <button class="primary" data-testid="open-video" onclick={onOpen}>Open video…</button>
  {#if store.video}
    <span class="file" title={store.video.info.path} data-testid="video-name">
      {basename(store.video.info.path)}
      <span class="muted">
        {store.video.info.width}×{store.video.info.height} · {store.fps.toFixed(2)} fps · {store
          .video.info.frame_count} frames
      </span>
    </span>
  {/if}
  <span class="spacer"></span>
  {#if job?.state === 'running'}
    <button class="progress" data-testid="render-progress-chip" onclick={onRender}>
      Rendering {Math.floor((job.done / Math.max(1, job.total)) * 100)}%
    </button>
  {/if}
  <button data-testid="load-preset" onclick={onLoadPreset}>Load preset</button>
  <button data-testid="save-preset" onclick={onSavePreset}>Save preset</button>
  <button data-testid="open-settings" onclick={onSettings}>Settings</button>
  <button class="primary" data-testid="open-render" disabled={!store.video} onclick={onRender}>
    Render…
  </button>
  <span
    class="status"
    class:online={store.connected}
    title={store.connected ? 'Connected' : 'Disconnected'}
    data-testid="connection"
    data-connected={store.connected}
  ></span>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }

  .logo {
    font-weight: 600;
    margin-right: 8px;
  }

  .file {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .spacer {
    flex: 1;
  }

  .progress {
    border-color: var(--accent);
  }

  .status {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--danger);
    flex-shrink: 0;
  }

  .status.online {
    background: var(--ok);
  }
</style>
