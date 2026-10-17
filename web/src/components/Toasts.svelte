<script lang="ts">
  import { store } from '../lib/store.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each store.toasts as t (t.id)}
    <div class="toast {t.kind}" data-testid="toast-{t.kind}">
      <span>{t.text}</span>
      <button class="icon" aria-label="Dismiss" onclick={() => store.dismiss(t.id)}>✕</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 20;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-left: 4px solid var(--accent);
    border-radius: var(--radius);
    padding: 8px 8px 8px 12px;
    max-width: min(640px, calc(100vw - 32px));
    box-shadow: 0 6px 20px rgb(0 0 0 / 40%);
  }

  .toast.error {
    border-left-color: var(--danger);
  }

  .toast button {
    border: none;
    background: none;
  }
</style>
