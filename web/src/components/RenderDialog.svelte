<script lang="ts">
  import { untrack } from 'svelte';
  import { api, ApiError, downloadUrl } from '../lib/api';
  import { joinArgs, splitArgs } from '../lib/args';
  import { defaultOutput, withExtension } from '../lib/paths';
  import { store } from '../lib/store.svelte';
  import { formatDuration } from '../lib/time';
  import { CODECS, type Codec, type RenderOptions } from '../lib/types';
  import Modal from './Modal.svelte';

  let {
    onClose,
    pickOutput,
  }: { onClose: () => void; pickOutput: (suggested: string) => Promise<string | null> } = $props();

  const opts = $derived(store.preset.render);
  const ext = (c: Codec) => CODECS.find((x) => x.value === c)?.ext ?? 'mp4';
  let output = $state(
    untrack(() =>
      store.video ? defaultOutput(store.video.info.path, ext(store.preset.render.codec)) : '',
    ),
  );
  let command = $state('');
  let confirmOverwrite = $state(false);
  let extra = $state(untrack(() => joinArgs(store.preset.render.extra_args)));

  const CRF_DEFAULT: Partial<Record<Codec, number>> = { h264: 18, h265: 20, vp9: 31 };
  const PRESETS: Partial<Record<Codec, { value: string; label: string }[]>> = {
    h264: [
      'ultrafast',
      'superfast',
      'veryfast',
      'faster',
      'fast',
      'medium',
      'slow',
      'slower',
      'veryslow',
    ].map((v) => ({ value: v, label: v })),
    vp9: [0, 1, 2, 3, 4, 5, 6, 7, 8].map((v) => ({ value: String(v), label: `cpu-used ${v}` })),
    prores: ['Proxy', 'LT', 'Standard', 'HQ', '4444', '4444 XQ'].map((l, i) => ({
      value: String(i),
      label: l,
    })),
  };
  PRESETS.h265 = PRESETS.h264;
  const HEIGHTS = [2160, 1440, 1080, 720, 540, 480, 360, 240, 180, 120];

  function update(patch: Partial<RenderOptions>) {
    store.updatePreset({ render: patch });
  }

  function setCodec(codec: Codec) {
    const old = ext(opts.codec);
    if (output) {
      const hasOld = old ? output.toLowerCase().endsWith(`.${old}`) : !/\.[a-z0-9]+$/i.test(output);
      if (hasOld) output = withExtension(output, ext(codec));
    }
    update({ codec, crf: null, encoder_preset: null, pix_fmt: null });
  }

  function num(raw: string): number | null {
    if (raw.trim() === '') return null;
    const v = Number(raw);
    return Number.isFinite(v) && v >= 0 ? Math.round(v) : null;
  }

  // Preview of the ffmpeg command, refreshed (debounced) when settings change.
  $effect(() => {
    const path = output;
    void JSON.stringify(store.preset.render);
    if (!store.video) return;
    const t = setTimeout(async () => {
      try {
        command = (await api.renderCommand(path)).command;
      } catch (e) {
        command = e instanceof Error ? `⚠ ${e.message}` : String(e);
      }
    }, 150);
    return () => clearTimeout(t);
  });

  async function browse() {
    const p = await pickOutput(output);
    if (p) output = p;
  }

  async function start(overwrite: boolean) {
    confirmOverwrite = false;
    try {
      store.setJob(await api.startRender(output, overwrite));
    } catch (e) {
      if (e instanceof ApiError && e.status === 409 && e.message.includes('exists')) {
        confirmOverwrite = true;
      } else {
        store.error(e);
      }
    }
  }

  async function cancel() {
    try {
      await api.cancelRender();
    } catch (e) {
      store.error(e);
    }
  }

  const job = $derived(store.job);
  const running = $derived(job?.state === 'running');
  const count = $derived(store.frameCount);
</script>

<Modal title="Render" testid="render-dialog" wide {onClose}>
  <div class="grid">
    <label for="out">Output</label>
    <div class="line">
      <input id="out" type="text" bind:value={output} data-testid="render-output" />
      <button data-testid="render-browse" onclick={browse}>Browse…</button>
    </div>

    <label for="codec">Format</label>
    <select
      id="codec"
      value={opts.codec}
      data-testid="render-codec"
      onchange={(e) => setCodec(e.currentTarget.value as Codec)}
    >
      {#each CODECS as c (c.value)}
        <option value={c.value}>{c.label}</option>
      {/each}
    </select>

    {#if CRF_DEFAULT[opts.codec] !== undefined}
      <label for="crf">Quality (CRF)</label>
      <div class="line">
        <input
          id="crf"
          type="number"
          min="0"
          max="63"
          placeholder={String(CRF_DEFAULT[opts.codec])}
          value={opts.crf ?? ''}
          data-testid="render-crf"
          onchange={(e) => update({ crf: num(e.currentTarget.value) })}
        />
        <span class="muted">lower = better quality, larger file</span>
      </div>
    {/if}

    {#if PRESETS[opts.codec]}
      <label for="preset">{opts.codec === 'prores' ? 'Profile' : 'Speed'}</label>
      <select
        id="preset"
        value={opts.encoder_preset ?? ''}
        data-testid="render-encoder-preset"
        onchange={(e) => update({ encoder_preset: e.currentTarget.value || null })}
      >
        <option value="">Default</option>
        {#each PRESETS[opts.codec] ?? [] as p (p.value)}
          <option value={p.value}>{p.label}</option>
        {/each}
      </select>
    {/if}

    <label for="height">Resolution</label>
    <select
      id="height"
      value={opts.height === null ? '' : String(opts.height)}
      data-testid="render-height"
      onchange={(e) => update({ height: num(e.currentTarget.value) })}
    >
      <option value=""
        >Native{store.video
          ? ` (${store.video.info.width}×${store.video.info.height})`
          : ''}</option
      >
      {#each HEIGHTS.filter((h) => !store.video || h < store.video.info.height) as h (h)}
        <option value={String(h)}>{h}p</option>
      {/each}
    </select>

    <label for="start">Trim</label>
    <div class="line">
      <input
        id="start"
        type="number"
        min="0"
        max={count - 1}
        placeholder="0"
        value={opts.start ?? ''}
        data-testid="render-start"
        onchange={(e) => update({ start: num(e.currentTarget.value) })}
      />
      <button data-testid="render-start-current" onclick={() => update({ start: store.frame })}
        >In = current</button
      >
      <span class="muted">to</span>
      <input
        type="number"
        min="1"
        max={count}
        placeholder={String(count)}
        value={opts.end ?? ''}
        aria-label="End frame (exclusive)"
        data-testid="render-end"
        onchange={(e) => update({ end: num(e.currentTarget.value) })}
      />
      <button data-testid="render-end-current" onclick={() => update({ end: store.frame + 1 })}
        >Out = current</button
      >
      <button data-testid="render-range-clear" onclick={() => update({ start: null, end: null })}
        >All</button
      >
    </div>

    <label for="audio">Audio</label>
    <select
      id="audio"
      value={opts.audio}
      disabled={!store.video?.info.has_audio || opts.codec === 'png'}
      data-testid="render-audio"
      onchange={(e) => update({ audio: e.currentTarget.value as RenderOptions['audio'] })}
    >
      <option value="encode">Include (re-encode)</option>
      <option value="copy">Include (copy stream)</option>
      <option value="none">None</option>
    </select>

    <label for="pixfmt">Pixel format</label>
    <input
      id="pixfmt"
      type="text"
      placeholder="default"
      value={opts.pix_fmt ?? ''}
      data-testid="render-pix-fmt"
      onchange={(e) => update({ pix_fmt: e.currentTarget.value.trim() || null })}
    />

    <label for="extra">Extra ffmpeg args</label>
    <input
      id="extra"
      type="text"
      placeholder="e.g. -tune grain"
      bind:value={extra}
      data-testid="render-extra-args"
      onchange={() => update({ extra_args: splitArgs(extra) })}
    />
  </div>

  <div class="command">
    <span class="muted">Command</span>
    <pre class="mono" data-testid="render-command">{command}</pre>
  </div>

  {#if job}
    <div class="job" data-testid="render-job" data-state={job.state}>
      <progress max={Math.max(1, job.total)} value={job.done}></progress>
      <span data-testid="render-status">
        {#if job.state === 'running'}
          {job.done} / {job.total} frames · {job.fps.toFixed(1)} fps · ETA {formatDuration(
            job.eta_seconds,
          )}
        {:else if job.state === 'finished'}
          Finished: {job.output} ({formatDuration(job.elapsed_seconds)})
          {#if !store.info?.dialogs && job.output && opts.codec !== 'png'}
            <a class="download" href={downloadUrl(job.output)} data-testid="render-download"
              >Download</a
            >
          {/if}
        {:else if job.state === 'cancelled'}
          Cancelled
        {:else}
          Failed: {job.error}
        {/if}
      </span>
    </div>
  {/if}

  {#if confirmOverwrite}
    <div class="confirm" data-testid="render-confirm">
      <span>{output} already exists. Overwrite it?</span>
      <button data-testid="render-overwrite" onclick={() => start(true)}>Overwrite</button>
      <button onclick={() => (confirmOverwrite = false)}>Keep</button>
    </div>
  {/if}

  {#snippet footer()}
    {#if running}
      <button data-testid="render-cancel" onclick={cancel}>Cancel render</button>
    {/if}
    <button data-testid="render-close" onclick={onClose}>Close</button>
    <button
      class="primary"
      data-testid="render-start-button"
      disabled={running || !output || !store.video}
      onclick={() => start(false)}>Render</button
    >
  {/snippet}
</Modal>

<style>
  .grid {
    display: grid;
    grid-template-columns: 130px 1fr;
    gap: 8px 10px;
    align-items: center;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .line input[type='text'] {
    flex: 1;
    min-width: 0;
  }

  .command {
    margin-top: 14px;
  }

  pre {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px;
    white-space: pre-wrap;
    word-break: break-all;
    font-size: 11px;
    margin: 4px 0 0;
    max-height: 110px;
    overflow: auto;
  }

  .job {
    margin-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  progress {
    width: 100%;
  }

  .download {
    margin-left: 8px;
    color: var(--accent);
  }

  .confirm {
    margin-top: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--danger);
    border-radius: var(--radius);
  }

  .confirm span {
    flex: 1;
  }
</style>
