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
      let i = Math.floor((s.ts - start) / 3600);
      // A sample stamped this very second lands one past the end: samples are stamped as
      // a check ends and the chart reloads them moments later, so the newest was dropped
      // and a first check drew nothing under its own peak (D-281).
      if (i === hours) i = hours - 1;
      if (i < 0 || i >= hours) continue;
      out[i] = Math.max(out[i] ?? 0, s.players);
    }
    return out;
  });
  const top = $derived(Math.max(maxPlayers, ...buckets.map((b) => b ?? 0), 1));
  /** Runs of consecutive sampled hours, as [x, y] points. With a one-hour span the
   *  x maths was 0/0 and every point NaN, so a server first seen within the hour drew
   *  nothing; and a run of one hour is a lone point, which a polyline does not stroke
   *  (D-256). The template draws those as dots. */
  const segments = $derived.by(() => {
    const segs: [number, number][][] = [];
    let cur: [number, number][] = [];
    buckets.forEach((b, i) => {
      if (b == null) {
        if (cur.length) segs.push(cur);
        cur = [];
        return;
      }
      const x = hours > 1 ? (i / (hours - 1)) * W : W / 2;
      const y = H - 2 - (b / top) * (H - 6);
      cur.push([x, y]);
    });
    if (cur.length) segs.push(cur);
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
    {#each segments as seg, i (i)}
      {@const lone = seg.length === 1 ? seg[0] : undefined}
      {#if lone}
        <circle cx={lone[0]} cy={lone[1]} r="1.75" class="dot" />
      {:else}
        <polyline points={seg.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ")} class="line" />
      {/if}
    {/each}
  </svg>
  <!-- The peak is named under the chart by the details pane, with its time (D-248). -->
  <p class="muted small">{count} of {hours} hour{hours === 1 ? "" : "s"} sampled</p>
{/if}

<style>
  svg { display: block; overflow: visible; }
  .axis { stroke: var(--border); stroke-width: 1; }
  .line { fill: none; stroke: var(--accent); stroke-width: 1.5; stroke-linejoin: round; stroke-linecap: round; }
  .dot { fill: var(--accent); }
  .small { font-size: 11.5px; margin: 2px 0 0; }
</style>
