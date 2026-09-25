<script lang="ts" module>
  export type RailIconName = "news" | "servers" | "lan" | "favourites" | "friends" | "recent" | "mods" | "settings" | "logs" | "update";
</script>

<script lang="ts">
  // The section rail's icons (D-250): one 24-unit grid, one stroke, round ends, drawn in
  // `currentColor` so they take the item's accent. They replaced Unicode glyphs whose
  // size and weight changed with whichever font happened to have them. Drawn here
  // rather than taken from an icon set: nothing is added to the bundle but these paths.
  let { name }: { name: RailIconName } = $props();
</script>

<svg class="icon" viewBox="0 0 24 24" aria-hidden="true">
  {#if name === "news"}
    <rect x="4" y="4" width="16" height="16" rx="2.5" />
    <path d="M8 8.5h8M8 12h3M8 15.5h3" />
    <rect x="14" y="12" width="2" height="3.5" rx=".5" />
  {:else if name === "servers"}
    <rect x="3.5" y="4" width="17" height="7" rx="2" />
    <rect x="3.5" y="13" width="17" height="7" rx="2" />
    <path class="dot" d="M7.5 7.5h.01M7.5 16.5h.01" />
  {:else if name === "lan"}
    <rect x="9" y="3.5" width="6" height="5" rx="1.2" />
    <rect x="3" y="15.5" width="6" height="5" rx="1.2" />
    <rect x="15" y="15.5" width="6" height="5" rx="1.2" />
    <path d="M12 8.5V12M6 15.5V12h12v3.5" />
  {:else if name === "favourites"}
    <path d="M12 3.7L14.29 9.54L20.56 9.92L15.71 13.91L17.29 19.98L12 16.6L6.71 19.98L8.29 13.91L3.44 9.92L9.71 9.54Z" />
  {:else if name === "friends"}
    <circle cx="9" cy="8.5" r="3.5" />
    <path d="M3 20c0-3.3 2.7-6 6-6s6 2.7 6 6" />
    <path d="M15.5 5.2a3.5 3.5 0 0 1 0 6.6M17.5 14.3c2.1.7 3.5 2.9 3.5 5.7" />
  {:else if name === "recent"}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3 2" />
  {:else if name === "mods"}
    <path d="M12 3.2 20 7.6v8.8L12 20.8 4 16.4V7.6z" />
    <path d="M4 7.6l8 4.4 8-4.4M12 12v8.8" />
  {:else if name === "settings"}
    <path d="M10.37 5.19L10.64 2.9L13.36 2.9L13.63 5.19A7 7 0 0 1 15.66 6.03L17.47 4.6L19.4 6.53L17.97 8.34A7 7 0 0 1 18.81 10.37L21.1 10.64L21.1 13.36L18.81 13.63A7 7 0 0 1 17.97 15.66L19.4 17.47L17.47 19.4L15.66 17.97A7 7 0 0 1 13.63 18.81L13.36 21.1L10.64 21.1L10.37 18.81A7 7 0 0 1 8.34 17.97L6.53 19.4L4.6 17.47L6.03 15.66A7 7 0 0 1 5.19 13.63L2.9 13.36L2.9 10.64L5.19 10.37A7 7 0 0 1 6.03 8.34L4.6 6.53L6.53 4.6L8.34 6.03A7 7 0 0 1 10.37 5.19Z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === "logs"}
    <rect x="3" y="4.5" width="18" height="15" rx="2.5" />
    <path d="M7 9.5l3 2.5-3 2.5M12.5 15h4.5" />
  {:else}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 16V8.5M8.5 11.5 12 8l3.5 3.5" />
  {/if}
</svg>

<style>
  .icon { flex: none; display: block; width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  /* Status lights on the rack: a dot needs more ink than a line to read at 18 px. */
  .dot { stroke-width: 2.6; }
</style>
