<script lang="ts">
  // Country flag from the sprite built by tools/flags_build.js (flag-icons, MIT; D-073).
  // 16×12 px on screen from a 32×24 sprite cell, so it stays crisp on high-DPI screens.
  // Unknown or missing codes render nothing.
  import flagsUrl from "../assets/flags.png";
  import codes from "./flags.json";
  import { countryName } from "./types";

  /** `decorative`: the country's name is already written beside the flag, so a screen
   *  reader heard it twice ("Germany Germany", D-291). */
  let { code, size = 16, decorative = false }: { code: string | null | undefined; size?: number; decorative?: boolean } = $props();

  const index = $derived(code ? codes.indexOf(code.toLowerCase()) : -1);
  const name = $derived(countryName(code));
</script>

{#if index >= 0}
  <span
    class="flag"
    role={decorative ? undefined : "img"}
    aria-label={decorative ? undefined : name}
    aria-hidden={decorative ? "true" : undefined}
    title={name}
    style="width: {size}px; height: {(size * 3) / 4}px; background-image: url({flagsUrl}); background-size: auto {(size * 3) / 4}px; background-position: -{index * size}px 0"
  ></span>
{/if}

<style>
  .flag { display: inline-block; flex: none; vertical-align: -1px; border-radius: 2px; background-repeat: no-repeat; box-shadow: 0 0 0 1px color-mix(in srgb, var(--fg) 12%, transparent); }
</style>
