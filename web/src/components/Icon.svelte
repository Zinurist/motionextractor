<script lang="ts">
  // Inline icons, so they do not depend on the fonts installed on the system.
  let {
    name,
  }: { name: 'start' | 'prev' | 'play' | 'pause' | 'next' | 'end' | 'folder' | 'film' | 'file' } =
    $props();
</script>

<svg viewBox="0 0 16 16" width="14" height="14" fill="currentColor" aria-hidden="true">
  {#if name === 'start'}
    <rect x="2" y="3" width="2" height="10" /><path d="M14 3v10L5 8z" />
  {:else if name === 'prev'}
    <path d="M12 3v10L3 8z" />
  {:else if name === 'play'}
    <path d="M4 2.5v11L13.5 8z" />
  {:else if name === 'pause'}
    <rect x="3.5" y="3" width="3" height="10" /><rect x="9.5" y="3" width="3" height="10" />
  {:else if name === 'next'}
    <path d="M4 3v10l9-5z" />
  {:else if name === 'end'}
    <rect x="12" y="3" width="2" height="10" /><path d="M2 3v10l9-5z" />
  {:else if name === 'folder'}
    <path d="M1.5 3.5h5l1.5 1.5h6.5v8h-13z" />
  {:else if name === 'film'}
    <path
      d="M2 2.5h12v11H2zm1.5 1v1.5H5V3.5zm7.5 0v1.5h1.5V3.5zM3.5 7v2H5V7zm7.5 0v2h1.5V7zm-7.5 3.5V12H5v-1.5zm7.5 0V12h1.5v-1.5zM6 3.5v9h4v-9z"
      fill-rule="evenodd"
    />
  {:else}
    <path d="M3 1.5h6.5L13 5v9.5H3z" opacity="0.6" />
  {/if}
</svg>
