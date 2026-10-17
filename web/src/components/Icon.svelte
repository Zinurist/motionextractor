<script lang="ts">
  // Inline icons, so they do not depend on the fonts installed on the system.
  let {
    name,
  }: {
    name:
      | 'start'
      | 'prev'
      | 'play'
      | 'pause'
      | 'next'
      | 'end'
      | 'folder'
      | 'film'
      | 'file'
      | 'reset'
      | 'volume'
      | 'muted'
      | 'download'
      | 'upload';
  } = $props();
</script>

<svg viewBox="0 0 16 16" width="14" height="14" fill="currentColor" aria-hidden="true">
  {#if name === 'start'}
    <rect x="1.5" y="3" width="2" height="10" /><path d="M9 3v10L3.5 8zM15 3v10L9.5 8z" />
  {:else if name === 'prev'}
    <rect x="3" y="3.5" width="2" height="9" /><path d="M13 3.5v9L6 8z" />
  {:else if name === 'play'}
    <path d="M4 2.5v11L13.5 8z" />
  {:else if name === 'pause'}
    <rect x="3.5" y="3" width="3" height="10" /><rect x="9.5" y="3" width="3" height="10" />
  {:else if name === 'next'}
    <rect x="11" y="3.5" width="2" height="9" /><path d="M3 3.5v9L10 8z" />
  {:else if name === 'end'}
    <rect x="12.5" y="3" width="2" height="10" /><path d="M1 3v10l5.5-5zM7 3v10l5.5-5z" />
  {:else if name === 'reset'}
    <path
      d="M8 2.5a5.5 5.5 0 1 1-5.2 3.7"
      fill="none"
      stroke="currentColor"
      stroke-width="1.6"
      stroke-linecap="round"
    />
    <path d="M1.6 2.2 6 3.4 3 6.8z" />
  {:else if name === 'volume' || name === 'muted'}
    <path d="M2 6h3l4-3.5v11L5 10H2z" />
    {#if name === 'volume'}
      <path
        d="M11 5.5a3.5 3.5 0 0 1 0 5M12.5 3.5a6.5 6.5 0 0 1 0 9"
        fill="none"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
      />
    {:else}
      <path
        d="M11 6l4 4M15 6l-4 4"
        fill="none"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
      />
    {/if}
  {:else if name === 'download' || name === 'upload'}
    <path
      d={name === 'download' ? 'M8 2v8M4.5 6.5 8 10l3.5-3.5' : 'M8 10V2M4.5 5.5 8 2l3.5 3.5'}
      fill="none"
      stroke="currentColor"
      stroke-width="1.6"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
    <path d="M2.5 11v2.5h11V11" fill="none" stroke="currentColor" stroke-width="1.6" />
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
