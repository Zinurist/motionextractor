<script lang="ts">
  let {
    label,
    value,
    min,
    max,
    step,
    testid,
    disabled = false,
    format,
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
    onChange: (v: number) => void;
  } = $props();

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
  {#if format}<span class="muted unit">{format(value)}</span>{/if}
</div>

<style>
  .slider {
    display: grid;
    grid-template-columns: 92px 1fr 64px;
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
    grid-column: 2 / 4;
    font-size: 11px;
    margin-top: -4px;
  }
</style>
