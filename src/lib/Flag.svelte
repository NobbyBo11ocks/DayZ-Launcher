<script lang="ts">
  // Country flag from the sprite built by tools/flags_build.js (flag-icons, MIT; D-073).
  // 16×12 px on screen from a 32×24 sprite cell, so it stays crisp on high-DPI screens.
  // Unknown or missing codes render nothing.
  import flagsUrl from "../assets/flags.png";
  import codes from "./flags.json";
  import { countryName } from "./types";

  let { code, size = 16 }: { code: string | null | undefined; size?: number } = $props();

  const index = $derived(code ? codes.indexOf(code.toLowerCase()) : -1);
  const name = $derived(countryName(code));
</script>

{#if index >= 0}
  <span
    class="flag"
    role="img"
    aria-label={name}
    title={name}
    style="width: {size}px; height: {(size * 3) / 4}px; background-image: url({flagsUrl}); background-size: auto {(size * 3) / 4}px; background-position: -{index * size}px 0"
  ></span>
{/if}

<style>
  .flag { display: inline-block; flex: none; vertical-align: -1px; border-radius: 2px; background-repeat: no-repeat; box-shadow: 0 0 0 1px color-mix(in srgb, var(--fg) 12%, transparent); }
</style>
