<script lang="ts">
  // Landing page (D-099, D-100): a welcome band with the Steam persona and avatar,
  // the newest post featured with its picture, then the latest posts as cards.
  // Pictures are host-made thumbnails (D-111). A video plays in a player that is
  // created on click and destroyed on close (D-150), so an idle home page still has
  // no embedded player and the memory budget holds.
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { SvelteSet } from "svelte/reactivity";
  import { news, type NewsView } from "./state/news.svelte";
  import { servers } from "./state/servers.svelte";
  import { updates } from "./state/updates.svelte";
  import type { NewsItem } from "./types";

  $effect(() => {
    news.beginVisit();
    return () => news.endVisit();
  });
  $effect(() => {
    if (news.items.length) news.markSeen();
  });

  const VIEWS: { id: NewsView; label: string; title: string }[] = [
    { id: "updates", label: "Updates", title: "Game updates, hotfixes, experimental and stable releases" },
    { id: "official", label: "All news", title: "Every post from Bohemia: updates, dev blogs, sales" },
    { id: "press", label: "With press", title: "Also the press feeds Steam attaches to DayZ" },
  ];

  const featured = $derived(news.list[0] ?? null);
  const rest = $derived(news.list.slice(1, 25));

  const day = (unix: number) => new Date(unix * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  const isNew = (n: NewsItem) => n.official && n.date > news.visitSeen;
  /** Posts whose picture failed to load: the card falls back to text (a broken image is worse than none). */
  const broken = new SvelteSet<string>();
  const thumb = (n: NewsItem, big = false) => (broken.has(n.gid) ? null : news.thumbUrl(n, big));
  const watch = (n: NewsItem) => `https://www.youtube.com/watch?v=${n.video}`;

  // The video open in the player, or null. `-nocookie` is YouTube's no-tracking host and
  // the only frame source the CSP allows (D-150).
  let playing = $state<{ id: string; title: string } | null>(null);
  let playerEl = $state<HTMLDivElement | null>(null);
  let closeBtn = $state<HTMLButtonElement | null>(null);
  let returnFocus: HTMLElement | null = null;

  /** Closes the player and hands focus back to whatever opened it (D-224). */
  function closePlayer() {
    playing = null;
    returnFocus?.focus();
    returnFocus = null;
  }

  $effect(() => {
    if (!playing) return;
    closeBtn?.focus();
  });

  /** Keeps Tab inside the overlay, the way Welcome and JoinDialog already do. */
  function trap(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      closePlayer();
      return;
    }
    if (e.key !== "Tab" || !playerEl) return;
    const focusable = playerEl.querySelectorAll<HTMLElement>("button, iframe, [href], [tabindex]:not([tabindex='-1'])");
    if (focusable.length === 0) return;
    const first = focusable[0]!;
    const last = focusable[focusable.length - 1]!;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
  const embed = (id: string) => `https://www.youtube-nocookie.com/embed/${id}?autoplay=1&rel=0&modestbranding=1`;
  function play(n: NewsItem) {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (n.video) playing = { id: n.video, title: n.title };
  }
  function onPlayerKey(e: KeyboardEvent) {
    if (e.key === "Escape" && playing) {
      e.stopPropagation();
      playing = null;
    }
  }

  function open(url: string) {
    void openUrl(url).catch((e) => (news.error = String(e)));
  }
</script>

<section class="home">
  <div class="bar">
    <h2>{news.view === "updates" ? "Latest updates" : "Latest from DayZ"}</h2>
    <div class="seg" role="group" aria-label="Which posts to show">
      {#each VIEWS as v (v.id)}
        <button class="segbtn" class:on={news.view === v.id} aria-pressed={news.view === v.id} title={v.title} onclick={() => (news.view = v.id)}>{v.label}</button>
      {/each}
    </div>
    {#if news.error}<span class="error">{news.error}</span>{/if}
  </div>

  <div class="scroll">
    {#if featured}
      <article class="featured" class:update={featured.update}>
        {#if thumb(featured, true)}
          <button class="media" onclick={() => (featured.video ? play(featured) : open(featured.url))} aria-label={featured.video ? "Play the video" : "Open the post"}>
            <img src={thumb(featured, true)} alt="" onerror={() => broken.add(featured.gid)} />
            {#if featured.video}<span class="play" aria-hidden="true"></span>{/if}
          </button>
        {/if}
        <div class="body">
          <div class="meta">
            <span>{day(featured.date)}</span>
            <span class="dot">·</span>
            <span>{featured.feed}</span>
            {#if featured.update}<span class="pill up">Update</span>{/if}
            {#if isNew(featured)}<span class="pill new">New</span>{/if}
          </div>
          <h3><button class="link" onclick={() => open(featured.url)} title={featured.url}>{featured.title}</button></h3>
          {#if featured.summary}<p class="summary clamp feat">{featured.summary}</p>{/if}
          <div class="links">
            <button class="btn small" onclick={() => open(featured.url)}>Read the full post</button>
            {#if featured.video}
              <button class="btn small secondary" onclick={() => play(featured)}>Play video</button>
              <button class="btn small secondary" onclick={() => open(watch(featured))} title="Open it in your browser instead">On YouTube</button>
            {/if}
          </div>
        </div>
      </article>
      <div class="grid">
        {#each rest as n (n.gid)}
          <article class="card" class:update={n.update}>
            {#if thumb(n)}
              <button class="media" onclick={() => (n.video ? play(n) : open(n.url))} aria-label={n.video ? "Play the video" : "Open the post"}>
                <img src={thumb(n)} alt="" loading="lazy" onerror={() => broken.add(n.gid)} />
                {#if n.video}<span class="play small" aria-hidden="true"></span>{/if}
              </button>
            {/if}
            <div class="body">
              <div class="meta">
                <span>{day(n.date)}</span>
                <span class="dot">·</span>
                <span>{n.feed}</span>
                {#if n.update}<span class="pill up">Update</span>{/if}
                {#if isNew(n)}<span class="pill new">New</span>{/if}
              </div>
              <h3><button class="link" onclick={() => open(n.url)} title={n.url}>{n.title}</button></h3>
              {#if n.summary}<p class="summary clamp">{n.summary}</p>{/if}
            </div>
          </article>
        {/each}
      </div>
    {:else}
      <p class="muted">{news.loading ? "Loading the latest posts…" : "No posts to show."}</p>
    {/if}
  </div>
</section>

<!-- Player (D-150): the iframe exists only while a video is open, so an idle home page
     still carries no embedded player. Escape or the backdrop closes it. -->
{#if playing}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="player-backdrop" role="presentation" onclick={closePlayer}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="player"
      bind:this={playerEl}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label={playing.title}
      onclick={(e) => e.stopPropagation()}
      onkeydown={trap}
    >
      <div class="player-bar">
        <span class="player-title" title={playing.title}>{playing.title}</span>
        <button class="btn small secondary" onclick={() => open(`https://www.youtube.com/watch?v=${playing?.id}`)}>On YouTube</button>
        <button class="btn small secondary" bind:this={closeBtn} onclick={closePlayer} aria-label="Close the video">Close</button>
      </div>
      <!-- `frame-src` says what may be framed, not what the frame may do: without
           a sandbox the player could navigate the whole window away on a click.
           allow-top-navigation is deliberately absent (D-160). -->
      <iframe
        src={embed(playing.id)}
        title={playing.title}
        sandbox="allow-scripts allow-same-origin allow-presentation allow-popups allow-popups-to-escape-sandbox"
        allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen"
        allowfullscreen
        referrerpolicy="strict-origin-when-cross-origin"
      ></iframe>
    </div>
  </div>
{/if}

<svelte:window onkeydown={onPlayerKey} />

<style>
  .home { display: flex; flex-direction: column; gap: 12px; min-height: 0; }

  /* Welcome band: accent glow from the top-left corner, persona and quick facts. */


  /* Video player (D-150). */
  .player-backdrop { position: fixed; inset: 0; z-index: 60; display: grid; place-items: center; padding: 24px; background: rgb(0 0 0 / 0.72); }
  .player { width: min(1000px, 100%); display: flex; flex-direction: column; gap: 8px; }
  .player-bar { display: flex; align-items: center; gap: 8px; }
  .player-title { flex: 1; min-width: 0; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .player iframe { width: 100%; aspect-ratio: 16 / 9; max-height: 74vh; border: 0; border-radius: 12px; background: #000; box-shadow: 0 24px 60px rgb(0 0 0 / 0.5); }

  .bar { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; font-size: 12.5px; }
  .bar h2 { margin: 0 4px 0 0; font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.06em; }
  .seg { display: inline-flex; padding: 2px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border-control); }
  .segbtn { all: unset; cursor: pointer; padding: 4px 11px; border-radius: 8px; color: var(--fg-muted); font-size: 12px; font-weight: 500; }
  .segbtn:hover { color: var(--fg); }
  .segbtn.on { background: var(--accent); color: var(--accent-fg); font-weight: 600; }
  .segbtn:focus-visible { outline: 2px solid var(--accent-ink); }

  .scroll { flex: 1; min-height: 0; overflow: auto; padding-right: 4px; display: flex; flex-direction: column; gap: 12px; }
  /* The list scrolls; its children keep their natural height instead of shrinking
     (the featured card has overflow hidden, so it would otherwise collapse to 0). */
  .scroll > * { flex: none; }

  /* Pictures and video previews: a fixed 16:9 box, the image covers it; a video shows a play badge. */
  .media { all: unset; cursor: pointer; position: relative; display: block; width: 100%; aspect-ratio: 16 / 9; background: var(--bg-row); overflow: hidden; }
  .media img { width: 100%; height: 100%; object-fit: cover; display: block; transition: transform 150ms; }
  .media:hover img { transform: scale(1.03); }
  .media:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: -2px; }
  .play { position: absolute; inset: 0; margin: auto; width: 56px; height: 56px; border-radius: 50%; background: rgba(0, 0, 0, 0.55); border: 2px solid rgba(255, 255, 255, 0.85); backdrop-filter: blur(2px); }
  .play::after { content: ""; position: absolute; left: 21px; top: 16px; border-style: solid; border-width: 10px 0 10px 17px; border-color: transparent transparent transparent #fff; }
  .play.small { width: 40px; height: 40px; }
  .play.small::after { left: 15px; top: 11px; border-width: 7px 0 7px 12px; }

  .featured { display: grid; grid-template-columns: minmax(280px, 42%) minmax(0, 1fr); border-radius: 14px; overflow: hidden; border: 1px solid var(--border); background: var(--bg-elev); }
  .featured.update { border-color: color-mix(in srgb, var(--accent) 50%, var(--border)); }
  /* Capped so the hero cannot eat the window on a large screen; the grid below keeps
     more cards in view (D-143). */
  .featured { max-height: 320px; }
  .featured .media { aspect-ratio: auto; height: 100%; min-height: 200px; }
  .featured .body { padding: 18px 20px; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .featured h3 { margin: 0; font-size: 19px; font-weight: 650; line-height: 1.25; }
  .links { display: flex; gap: 8px; margin-top: auto; padding-top: 4px; flex-wrap: wrap; }

  .meta { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; font-size: 11.5px; color: var(--fg-muted); }
  .dot { opacity: 0.6; }
  .pill { padding: 1px 7px; border-radius: 9px; font-size: 10.5px; font-weight: 600; letter-spacing: 0.02em; }
  /* Accent ink needs a surface it can be read on: the 22% tint measured as low as
     2.22:1 in light theme (D-186), so the pill takes the plain row background. */
  .pill.up { background: var(--bg-row); color: var(--accent-ink); border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border)); }
  .pill.new { background: var(--accent); color: var(--accent-fg); }
  .link { all: unset; cursor: pointer; color: var(--fg); }
  .link:hover { color: var(--accent-ink); }
  .link:focus-visible { outline: 2px solid var(--accent-ink); border-radius: 4px; }
  .summary { margin: 0; font-size: 13px; color: var(--fg-muted); line-height: 1.5; overflow-wrap: anywhere; }
  .summary.clamp { display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .summary.feat { -webkit-line-clamp: 5; line-clamp: 5; }

  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; padding-bottom: 4px; }
  .card { display: flex; flex-direction: column; border-radius: 12px; overflow: hidden; background: var(--bg-elev); border: 1px solid var(--border); transition: transform 150ms, border-color 150ms; min-width: 0; }
  .card:hover { transform: translateY(-2px); border-color: color-mix(in srgb, var(--accent) 45%, var(--border)); }
  .card.update { box-shadow: inset 3px 0 0 var(--accent); }
  .card .body { padding: 12px 14px 14px; display: flex; flex-direction: column; gap: 6px; }
  .card h3 { margin: 0; font-size: 14px; font-weight: 600; line-height: 1.3; }
  .error { color: var(--danger); }

  @media (max-width: 1100px) {
    /* Stacked, the cap that keeps the two-column hero tidy hides everything below the
       picture — title, summary and all three buttons — across the whole 960–1100 px
       band. D-153 recorded this as fixed; it never was (D-197). */
    .featured { max-height: none; }
    .featured { grid-template-columns: 1fr; }
    .featured .media { aspect-ratio: 16 / 9; height: auto; min-height: 0; }
  }
</style>
