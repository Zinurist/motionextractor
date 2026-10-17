<script lang="ts">
  import Icon from './Icon.svelte';

  let {
    label,
    value,
    min,
    max,
    step,
    testid,
    disabled = false,
    format,
    defaultValue,
    onChange,
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    step: number;
    testid: string;
    disabled?: boolean;
    format?: (v: number) => string;
    /** When set, a reset button appears while the value differs from it. */
    defaultValue?: number;
    onChange: (v: number) => void;
  } = $props();

  const modified = $derived(defaultValue !== undefined && Math.abs(value - defaultValue) > 1e-9);

  function set(raw: string) {
    const v = Number(raw);
    if (raw !== '' && Number.isFinite(v)) onChange(Math.min(max, Math.max(min, v)));
  }
</script>

<div class="slider" class:disabled>
  <label for={testid}>{label}</label>
  <input
    id={testid}
    type="range"
    {min}
    {max}
    {step}
    {value}
    {disabled}
    data-testid={testid}
    oninput={(e) => set(e.currentTarget.value)}
  />
  <input
    type="number"
    {min}
    {max}
    {step}
    {value}
    {disabled}
    aria-label={label}
    data-testid={`${testid}-value`}
    onchange={(e) => set(e.currentTarget.value)}
  />
  <span class="reset-slot">
    {#if modified && defaultValue !== undefined}
      <button
        class="reset"
        title="Reset to {defaultValue}"
        aria-label="Reset {label}"
        data-testid={`${testid}-reset`}
        {disabled}
        onclick={() => onChange(defaultValue)}><Icon name="reset" /></button
      >
    {/if}
  </span>
  {#if format}<span class="muted unit">{format(value)}</span>{/if}
</div>

<style>
  .slider {
    display: grid;
    grid-template-columns: 92px 1fr 64px 22px;
    align-items: center;
    gap: 6px;
    margin: 4px 0;
  }

  .slider.disabled {
    opacity: 0.5;
  }

  input[type='number'] {
    width: 64px;
  }

  input[type='range'] {
    min-width: 0;
  }

  .unit {
    grid-column: 2 / 5;
    font-size: 11px;
    margin-top: -4px;
  }

  .reset-slot {
    display: flex;
    justify-content: center;
  }

  .reset {
    display: flex;
    align-items: center;
    padding: 2px;
    border: none;
    background: none;
    color: var(--muted);
  }

  .reset:hover:not(:disabled) {
    color: var(--text);
  }
</style>
