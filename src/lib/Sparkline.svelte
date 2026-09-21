<script lang="ts">
  // 72-hour population sparkline from local verified samples (docs/06 §3).
  // Hourly buckets (max per hour); missing hours are gaps, not zeros.
  import type { PopulationSample } from "./types";

  let { samples, maxPlayers, hours = 72 }: { samples: PopulationSample[]; maxPlayers: number; hours?: number } = $props();

  const W = 320;
  const H = 48;

  const buckets = $derived.by(() => {
    const now = Math.floor(Date.now() / 1000);
    const start = now - hours * 3600;
    const out: (number | null)[] = Array.from({ length: hours }, () => null);
    for (const s of samples) {
      const i = Math.floor((s.ts - start) / 3600);
      if (i < 0 || i >= hours) continue;
      out[i] = Math.max(out[i] ?? 0, s.players);
    }
    return out;
  });
  const top = $derived(Math.max(maxPlayers, ...buckets.map((b) => b ?? 0), 1));
  const points = $derived.by(() => {
    const segs: string[] = [];
    let cur: string[] = [];
    buckets.forEach((b, i) => {
      if (b == null) {
        if (cur.length) segs.push(cur.join(" "));
        cur = [];
        return;
      }
      const x = (i / (hours - 1)) * W;
      const y = H - 2 - (b / top) * (H - 6);
      cur.push(`${x.toFixed(1)},${y.toFixed(1)}`);
    });
    if (cur.length) segs.push(cur.join(" "));
    return segs;
  });
  const count = $derived(buckets.filter((b) => b != null).length);
  const peak = $derived(Math.max(0, ...buckets.map((b) => b ?? 0)));
</script>

{#if count === 0}
  <p class="muted small">No population samples yet. Samples are recorded while the launcher runs and verifies this server.</p>
{:else}
  <svg viewBox="0 0 {W} {H}" width="100%" height={H} role="img" aria-label="Players over the last {hours} hours, peak {peak} of {maxPlayers}">
    <line x1="0" y1={H - 2} x2={W} y2={H - 2} class="axis" />
    {#each points as d, i (i)}
      <polyline points={d} class="line" />
    {/each}
  </svg>
  <p class="muted small">Peak {peak}/{maxPlayers} · {count} of {hours} hours sampled</p>
{/if}

<style>
  svg { display: block; overflow: visible; }
  .axis { stroke: var(--border); stroke-width: 1; }
  .line { fill: none; stroke: var(--accent); stroke-width: 1.5; stroke-linejoin: round; stroke-linecap: round; }
  .small { font-size: 11.5px; margin: 2px 0 0; }
</style>
