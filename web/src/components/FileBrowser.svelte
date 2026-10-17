<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { api } from '../lib/api';
  import { basename, join } from '../lib/paths';
  import { store } from '../lib/store.svelte';
  import type { Listing } from '../lib/types';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  let {
    mode,
    title,
    start,
    suggested = '',
    onPick,
    onCancel,
  }: {
    mode: 'open' | 'save';
    title: string;
    start?: string;
    suggested?: string;
    onPick: (path: string) => void;
    onCancel: () => void;
  } = $props();

  let listing = $state<Listing | null>(null);
  let name = $state(untrack(() => (suggested ? basename(suggested) : '')));
  let error = $state('');
  const VIDEO = /\.(mp4|mov|mkv|webm|avi|m4v|ts|mts|m2ts|mpg|mpeg|wmv|flv|gif|ogv|3gp)$/i;

  async function load(path?: string) {
    try {
      listing = await api.list(path);
      error = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      if (path !== undefined && !listing) await load();
    }
  }

  onMount(() => {
    void load(start);
  });

  function choose(entry: Listing['entries'][number]) {
    if (entry.dir) void load(entry.path);
    else if (mode === 'open') onPick(entry.path);
    else name = entry.name;
  }

  function confirm() {
    if (!listing || !name.trim()) return;
    onPick(join(listing.path, name.trim()));
  }

  const roots = $derived(store.info?.roots ?? []);
</script>

<Modal {title} testid="file-browser" wide onClose={onCancel}>
  <div class="bar">
    <button
      class="icon"
      aria-label="Parent directory"
      data-testid="fb-up"
      disabled={!listing?.parent}
      onclick={() => listing?.parent && load(listing.parent)}>↑</button
    >
    <!-- Right-to-left so long paths are cut at the start; the inner span keeps the text order. -->
    <span class="mono path"
      ><span class="ltr" data-testid="fb-path">{listing?.path ?? ''}</span></span
    >
    {#if roots.length > 1}
      <select aria-label="Root" onchange={(e) => load(e.currentTarget.value)}>
        {#each roots as r (r)}<option value={r}>{r}</option>{/each}
      </select>
    {/if}
  </div>
  {#if error}<p class="error" data-testid="fb-error">{error}</p>{/if}
  <ul class="entries" data-testid="fb-entries">
    {#each listing?.entries ?? [] as e (e.path)}
      <li>
        <button
          class="entry"
          class:dir={e.dir}
          class:video={!e.dir && VIDEO.test(e.name)}
          data-testid="fb-entry"
          data-name={e.name}
          ondblclick={() => choose(e)}
          onclick={() => (e.dir || mode === 'save' ? choose(e) : onPick(e.path))}
        >
          <Icon name={e.dir ? 'folder' : VIDEO.test(e.name) ? 'film' : 'file'} />
          <span class="name">{e.name}</span>
          {#if !e.dir}<span class="muted size">{(e.size / 1048576).toFixed(1)} MB</span>{/if}
        </button>
      </li>
    {:else}
      <li class="muted empty">Empty directory</li>
    {/each}
  </ul>
  {#if mode === 'save'}
    <div class="bar">
      <label for="fb-name">Name</label>
      <input id="fb-name" type="text" bind:value={name} data-testid="fb-name" />
    </div>
  {/if}

  {#snippet footer()}
    <button onclick={onCancel}>Cancel</button>
    {#if mode === 'save'}
      <button class="primary" data-testid="fb-confirm" disabled={!name.trim()} onclick={confirm}
        >Choose</button
      >
    {/if}
  {/snippet}
</Modal>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .bar input {
    flex: 1;
  }

  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .ltr {
    direction: ltr;
    unicode-bidi: embed;
  }

  .entries {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
    height: 320px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
  }

  .entry {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    border: none;
    border-radius: 0;
    background: none;
    text-align: left;
    padding: 5px 10px;
  }

  .entry:hover {
    background: var(--panel-2);
  }

  .entry .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .entry.dir .name,
  .entry.video .name {
    color: var(--text);
  }

  .entry:not(.dir):not(.video) .name {
    color: var(--muted);
  }

  .empty {
    padding: 10px;
  }

  .error {
    color: var(--danger);
  }
</style>
