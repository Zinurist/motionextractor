<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { offsetFrames } from '../lib/time';
  import { defaultPreset, type TimeUnit } from '../lib/types';
  import Slider from './Slider.svelte';

  /** Defaults the reset buttons go back to. */
  const D = defaultPreset().effect;
  const fx = $derived(store.preset.effect);
  const cmp = $derived(store.preset.comparison);
  const fps = $derived(store.fps);
  const disabled = $derived(fx.blend === 'classic');

  // The last rolling offset is kept while the fixed mode is selected.
  let lastOffset = $state({ value: 1, unit: 'frames' as TimeUnit });
  $effect(() => {
    if (cmp.mode === 'rolling') lastOffset = { ...cmp.offset };
  });

  function setMode(mode: 'rolling' | 'fixed') {
    if (mode === cmp.mode) return;
    store.setComparison(
      mode === 'rolling' ? { mode, offset: lastOffset } : { mode, frame: store.frame },
    );
  }

  function setOffset(value: number, unit: TimeUnit) {
    if (!Number.isFinite(value) || value === 0) {
      store.toast('error', 'The offset must not be zero.');
      return;
    }
    store.setComparison({ mode: 'rolling', offset: { value, unit } });
  }

  function setUnit(unit: TimeUnit) {
    if (cmp.mode !== 'rolling' || unit === cmp.offset.unit) return;
    // Keep the same distance when switching units.
    const frames = offsetFrames(cmp.offset.value, cmp.offset.unit, fps);
    const value = unit === 'frames' ? frames : Math.round((frames / fps) * 1000) / 1000;
    setOffset(value, unit);
  }

  function setReference(frame: number) {
    if (!Number.isFinite(frame) || frame < 0) return;
    store.setComparison({ mode: 'fixed', frame: Math.round(frame) });
  }

  const offsetInfo = $derived.by(() => {
    if (cmp.mode !== 'rolling') return '';
    const k = offsetFrames(cmp.offset.value, cmp.offset.unit, fps);
    return cmp.offset.unit === 'frames'
      ? `= ${(k / fps).toFixed(3)} s`
      : `= ${k} frame${Math.abs(k) === 1 ? '' : 's'}`;
  });

  const pct = (v: number) => `${(v * 100).toFixed(1)}% of height`;
</script>

<aside class="panel" data-testid="params">
  <section>
    <h3>Comparison</h3>
    <div class="radio-row">
      <label>
        <input
          type="radio"
          name="mode"
          checked={cmp.mode === 'rolling'}
          data-testid="mode-rolling"
          onchange={() => setMode('rolling')}
        />
        Motion
      </label>
      <label>
        <input
          type="radio"
          name="mode"
          checked={cmp.mode === 'fixed'}
          data-testid="mode-fixed"
          onchange={() => setMode('fixed')}
        />
        Since a frame
      </label>
    </div>
    {#if cmp.mode === 'rolling'}
      <div class="row">
        <label for="offset">Offset</label>
        <input
          id="offset"
          type="number"
          step={cmp.offset.unit === 'frames' ? 1 : 0.01}
          value={cmp.offset.value}
          data-testid="offset-value"
          onchange={(e) => setOffset(Number(e.currentTarget.value), cmp.offset.unit)}
        />
        <select
          value={cmp.offset.unit}
          aria-label="Offset unit"
          data-testid="offset-unit"
          onchange={(e) => setUnit(e.currentTarget.value as TimeUnit)}
        >
          <option value="frames">frames</option>
          <option value="seconds">seconds</option>
        </select>
      </div>
      <p class="muted hint" data-testid="offset-info">{offsetInfo}</p>
    {:else}
      <div class="row">
        <label for="ref">Reference</label>
        <input
          id="ref"
          type="number"
          min="0"
          max={Math.max(0, store.frameCount - 1)}
          value={cmp.frame}
          data-testid="reference-frame"
          onchange={(e) => setReference(Number(e.currentTarget.value))}
        />
        <button data-testid="reference-current" onclick={() => setReference(store.frame)}>
          Use current
        </button>
      </div>
      <p class="muted hint">= {(cmp.frame / fps).toFixed(3)} s</p>
    {/if}
  </section>

  <section>
    <h3>Difference</h3>
    <div class="row">
      <label for="blend">Mode</label>
      <select
        id="blend"
        value={fx.blend}
        data-testid="blend"
        onchange={(e) =>
          store.updatePreset({
            effect: { blend: e.currentTarget.value as 'difference' | 'classic' },
          })}
      >
        <option value="difference">Difference</option>
        <option value="classic">Classic (invert &amp; blend)</option>
      </select>
    </div>
    <div class="row">
      <label for="metric">Metric</label>
      <select
        id="metric"
        value={fx.metric}
        {disabled}
        data-testid="metric"
        onchange={(e) =>
          store.updatePreset({ effect: { metric: e.currentTarget.value as 'luma' | 'max' } })}
      >
        <option value="luma">Luma</option>
        <option value="max">Max channel</option>
      </select>
    </div>
    <Slider
      label="Threshold"
      testid="threshold"
      defaultValue={D.threshold}
      min={0}
      max={0.5}
      step={0.005}
      value={fx.threshold}
      {disabled}
      onChange={(v) => store.updatePreset({ effect: { threshold: v } })}
    />
    <Slider
      label="Gain"
      testid="gain"
      defaultValue={D.gain}
      min={0}
      max={20}
      step={0.1}
      value={fx.gain}
      {disabled}
      onChange={(v) => store.updatePreset({ effect: { gain: v } })}
    />
    <Slider
      label="Gamma"
      testid="gamma"
      defaultValue={D.gamma}
      min={0.2}
      max={4}
      step={0.05}
      value={fx.gamma}
      {disabled}
      onChange={(v) => store.updatePreset({ effect: { gamma: v } })}
    />
    <Slider
      label="Denoise"
      testid="denoise"
      defaultValue={D.denoise}
      min={0}
      max={0.02}
      step={0.0005}
      value={fx.denoise}
      format={pct}
      onChange={(v) => store.updatePreset({ effect: { denoise: v } })}
    />
  </section>

  <section>
    <h3>Color</h3>
    <div class="row">
      <label for="color">Motion color</label>
      <input
        id="color"
        type="color"
        value={fx.color}
        {disabled}
        data-testid="color"
        oninput={(e) => store.updatePreset({ effect: { color: e.currentTarget.value } })}
      />
      <span class="mono muted">{fx.color}</span>
    </div>
  </section>

  <section>
    <h3>
      <label>
        <input
          type="checkbox"
          checked={fx.bloom.enabled}
          {disabled}
          data-testid="bloom-enabled"
          onchange={(e) =>
            store.updatePreset({ effect: { bloom: { enabled: e.currentTarget.checked } } })}
        />
        Bloom
      </label>
    </h3>
    <Slider
      label="Radius"
      testid="bloom-radius"
      defaultValue={D.bloom.radius}
      min={0}
      max={0.2}
      step={0.002}
      value={fx.bloom.radius}
      format={pct}
      disabled={disabled || !fx.bloom.enabled}
      onChange={(v) => store.updatePreset({ effect: { bloom: { radius: v } } })}
    />
    <Slider
      label="Intensity"
      testid="bloom-intensity"
      defaultValue={D.bloom.intensity}
      min={0}
      max={5}
      step={0.05}
      value={fx.bloom.intensity}
      disabled={disabled || !fx.bloom.enabled}
      onChange={(v) => store.updatePreset({ effect: { bloom: { intensity: v } } })}
    />
    <Slider
      label="Threshold"
      testid="bloom-threshold"
      defaultValue={D.bloom.threshold}
      min={0}
      max={0.95}
      step={0.01}
      value={fx.bloom.threshold}
      disabled={disabled || !fx.bloom.enabled}
      onChange={(v) => store.updatePreset({ effect: { bloom: { threshold: v } } })}
    />
  </section>

  <section>
    <h3>Background</h3>
    <div class="radio-row">
      <label>
        <input
          type="radio"
          name="bg"
          checked={fx.background === 'black'}
          {disabled}
          data-testid="background-black"
          onchange={() => store.updatePreset({ effect: { background: 'black' } })}
        />
        Black
      </label>
      <label>
        <input
          type="radio"
          name="bg"
          checked={fx.background === 'original'}
          {disabled}
          data-testid="background-original"
          onchange={() => store.updatePreset({ effect: { background: 'original' } })}
        />
        Original video
      </label>
    </div>
    <Slider
      label="Brightness"
      testid="background-brightness"
      defaultValue={D.background_brightness}
      min={0}
      max={1}
      step={0.01}
      value={fx.background_brightness}
      disabled={disabled || fx.background !== 'original'}
      onChange={(v) => store.updatePreset({ effect: { background_brightness: v } })}
    />
    <Slider
      label="Opacity"
      testid="overlay-opacity"
      defaultValue={D.overlay_opacity}
      min={0}
      max={1}
      step={0.01}
      value={fx.overlay_opacity}
      disabled={disabled || fx.background !== 'original'}
      onChange={(v) => store.updatePreset({ effect: { overlay_opacity: v } })}
    />
  </section>
</aside>

<style>
  .panel {
    width: 320px;
    flex-shrink: 0;
    overflow-y: auto;
    border-left: 1px solid var(--border);
    background: var(--panel);
    padding: 4px 12px 16px;
  }

  section {
    border-bottom: 1px solid var(--border);
    padding: 8px 0 10px;
  }

  section:last-child {
    border-bottom: none;
  }

  h3 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 4px 0 8px;
  }

  h3 label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 4px 0;
  }

  .row > label {
    width: 92px;
    flex-shrink: 0;
  }

  .radio-row {
    display: flex;
    gap: 14px;
    margin-bottom: 6px;
  }

  .hint {
    margin: 2px 0 0 98px;
    font-size: 11px;
  }

  input[type='color'] {
    width: 44px;
    height: 26px;
    padding: 0 2px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
</style>
