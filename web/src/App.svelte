<script lang="ts">
  import { onMount } from 'svelte';
  import { store } from './lib/store.svelte';
  import TopBar from './components/TopBar.svelte';
  import Viewer from './components/Viewer.svelte';
  import Timeline from './components/Timeline.svelte';
  import ParamsPanel from './components/ParamsPanel.svelte';
  import RenderDialog from './components/RenderDialog.svelte';
  import SettingsDialog from './components/SettingsDialog.svelte';
  import FileBrowser from './components/FileBrowser.svelte';
  import Toasts from './components/Toasts.svelte';
  import { api } from './lib/api';
  import { defaultOutput, dirname } from './lib/paths';

  let renderOpen = $state(false);
  let settingsOpen = $state(false);
  let browser = $state<{
    mode: 'open' | 'save';
    title: string;
    start?: string;
    suggested?: string;
    done: (path: string) => void;
  } | null>(null);

  onMount(() => {
    void store.init();
  });

  /** Native dialog in the desktop app, the built-in browser otherwise. */
  async function pickPath(
    kind: 'open-video' | 'save-video' | 'open-preset' | 'save-preset',
    title: string,
    suggested = '',
  ): Promise<string | null> {
    if (store.info?.dialogs) {
      try {
        return (await api.dialog(kind, suggested)).path;
      } catch (e) {
        store.error(e);
        return null;
      }
    }
    return new Promise((resolve) => {
      const start = store.video ? dirname(store.video.info.path) : undefined;
      browser = {
        mode: kind.startsWith('open') ? 'open' : 'save',
        title,
        start,
        suggested,
        done: (path) => {
          browser = null;
          resolve(path);
        },
      };
    });
  }

  async function openVideo() {
    const path = await pickPath('open-video', 'Open video');
    if (path) await store.open(path);
  }

  async function savePreset() {
    const suggested = store.video ? defaultOutput(store.video.info.path, 'json') : 'preset.json';
    const path = await pickPath('save-preset', 'Save preset', suggested);
    if (!path) return;
    try {
      await api.savePreset(path);
      store.toast('info', `Preset saved to ${path}`);
    } catch (e) {
      store.error(e);
    }
  }

  async function loadPreset() {
    const path = await pickPath('open-preset', 'Load preset');
    if (!path) return;
    try {
      const r = await api.loadPreset(path);
      store.preset = r.preset;
      store.rev = r.rev;
      store.requestViews();
    } catch (e) {
      store.error(e);
    }
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (
      t.closest('input, select, textarea, [contenteditable]') ||
      renderOpen ||
      settingsOpen ||
      browser
    ) {
      return;
    }
    const second = Math.max(1, Math.round(store.fps));
    switch (e.key) {
      case ' ':
        store.togglePlay();
        break;
      case 'ArrowLeft':
        store.step(e.shiftKey ? -second : -1);
        break;
      case 'ArrowRight':
        store.step(e.shiftKey ? second : 1);
        break;
      case 'Home':
        store.step(-store.frameCount);
        break;
      case 'End':
        store.step(store.frameCount);
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    store.toast(
      'info',
      'Use “Open video” to choose a file (drag & drop works in the desktop app).',
    );
  }
</script>

<svelte:window onkeydown={onKey} ondragover={(e) => e.preventDefault()} ondrop={onDrop} />

<div class="app">
  <TopBar
    onOpen={openVideo}
    onRender={() => (renderOpen = true)}
    onSettings={() => (settingsOpen = true)}
    onSavePreset={savePreset}
    onLoadPreset={loadPreset}
  />
  <main>
    <section class="stage">
      <Viewer />
      <Timeline />
    </section>
    <ParamsPanel />
  </main>
</div>

{#if renderOpen}
  <RenderDialog
    onClose={() => (renderOpen = false)}
    pickOutput={(suggested) => pickPath('save-video', 'Render to', suggested)}
  />
{/if}
{#if settingsOpen}
  <SettingsDialog onClose={() => (settingsOpen = false)} />
{/if}
{#if browser}
  <FileBrowser
    mode={browser.mode}
    title={browser.title}
    start={browser.start}
    suggested={browser.suggested}
    onPick={(p) => browser?.done(p)}
    onCancel={() => browser?.done('')}
  />
{/if}
<Toasts />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  main {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
</style>
