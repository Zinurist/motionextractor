<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    onClose,
    testid,
    wide = false,
    children,
    footer,
  }: {
    title: string;
    onClose: () => void;
    testid: string;
    wide?: boolean;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') onClose();
  }
</script>

<svelte:window onkeydown={onKey} />

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && onClose()}
>
  <div
    class="modal"
    class:wide
    role="dialog"
    aria-modal="true"
    aria-label={title}
    data-testid={testid}
  >
    <header>
      <h2>{title}</h2>
      <button class="icon" aria-label="Close" onclick={onClose}>✕</button>
    </header>
    <div class="body">
      {@render children()}
    </div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 55%);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }

  .modal {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    display: flex;
    flex-direction: column;
    box-shadow: 0 12px 40px rgb(0 0 0 / 50%);
  }

  .modal.wide {
    width: min(760px, calc(100vw - 32px));
  }

  header,
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
  }

  header {
    border-bottom: 1px solid var(--border);
  }

  footer {
    border-top: 1px solid var(--border);
    justify-content: flex-end;
  }

  h2 {
    flex: 1;
    margin: 0;
    font-size: 15px;
  }

  .body {
    padding: 14px;
    overflow: auto;
  }
</style>
