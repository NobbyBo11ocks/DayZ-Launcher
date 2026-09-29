<script lang="ts">
  // Landing page (D-099, D-100): the newest post featured with its picture, then the
  // latest posts as cards (the welcome band went in D-173 and D-178).
  // Pictures are host-made thumbnails (D-111). A video plays in a player that is
  // created on click and destroyed on close (D-150), so an idle home page still has
  // no embedded player and the memory budget holds.
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";
  import { openExternal } from "./external";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { news, type NewsView } from "./state/news.svelte";
  import type { NewsItem } from "./types";
  import { newsGrid } from "./newsgrid";

  $effect(() => {
    // Untracked: `beginVisit` reads the seen mark, so the effect depended on it, and
    // `markSeen` below moving the mark re-ran it — which re-froze the boundary past
    // every post, and no "New" pill ever showed (D-100, D-240).
    untrack(() => news.beginVisit());
    return () => news.endVisit();
  });
  $effect(() => {
    if (news.items.length) news.markSeen();
  });

  const VIEWS: { id: NewsView; label: string; title: string }[] = [
    { id: "updates", label: "Updates", title: "Game updates, hotfixes, experimental and stable releases" },
    { id: "official", label: "All news", title: "Every post from Bohemia: updates, dev blogs, sales" },
  ];

  const featured = $derived(news.list[0] ?? null);
  /** The grid's own width, for its columns: rows come out full at any window size
   *  (`newsGrid`, D-326). */
  let gridWidth = $state(0);
  const layout = $derived(newsGrid(gridWidth, news.list.length - 1));
  const rest = $derived(news.list.slice(1, 1 + layout.count));

  const day = (unix: number) => new Date(unix * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  const isNew = (n: NewsItem) => n.official && n.date > news.visitSeen;
  /** Posts whose picture failed to load: the card falls back to text (a broken image is worse than none). */
  const broken = new SvelteSet<string>();
  const thumb = (n: NewsItem, big = false) => (broken.has(n.gid) ? null : news.thumbUrl(n, big));

  /** Cards whose picture may be asked for: on screen, or within a screen of it. Every
   *  card asked on first render, so opening "All news" fetched some 17 pictures, ~78 MB
   *  of sources, for cards nobody scrolled to (row 24). */
  const near = new SvelteSet<string>();
  let observer: IntersectionObserver | null = null;
  function lazy(node: HTMLElement, gid: string) {
    if (typeof IntersectionObserver !== "function") {
      near.add(gid);
      return;
    }
    observer ??= new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (!e.isIntersecting) continue;
          const id = (e.target as HTMLElement).dataset.gid;
          if (id) near.add(id);
          observer?.unobserve(e.target);
        }
      },
      { root: node.closest(".scroll"), rootMargin: "100% 0px" },
    );
    node.dataset.gid = gid;
    observer.observe(node);
    return { destroy: () => observer?.unobserve(node) };
  }
  $effect(() => () => {
    observer?.disconnect();
    observer = null;
  });
  const cardThumb = (n: NewsItem) => (near.has(n.gid) ? thumb(n) : null);
  /** Whether the post has a picture to show, loaded or not: its box holds the place while
   *  it loads, so the text does not jump; one that failed leaves the post as text, and a
   *  featured post without one takes its card's whole width (row 24, approved). */
  const hasPic = (n: NewsItem, big = false) => !broken.has(n.gid) && (!!n.video || (!!n.image && !news.thumbFailed(n, big)));
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
  // Elevated (matched to an elevated Steam, D-119) the process already runs every
  // untrusted parser as administrator; the third-party player is the one thing that
  // need not join it, so a video opens in the browser instead (D-248).
  let elevated = $state(false);
  $effect(() => {
    invoke<{ elevated?: boolean }>("app_info")
      .then((i) => (elevated = i.elevated === true))
      .catch(() => {});
  });
  function play(n: NewsItem) {
    if (!n.video) return;
    if (elevated) {
      open(watch(n));
      return;
    }
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    playing = { id: n.video, title: n.title };
  }
  // Escape with focus outside the player (inside it, `trap` has already closed it).
  // Through closePlayer, so focus goes back to whatever opened the video (D-224,
  // D-256); it only cleared `playing`, which dropped focus to <body>.
  function onPlayerKey(e: KeyboardEvent) {
    if (e.key === "Escape" && playing) {
      // Handled, so the notices leave it alone (row 27).
      e.preventDefault();
      e.stopPropagation();
      closePlayer();
    }
  }

  /** Why a post could not be opened. It went into the feed's error, whose retry then
   *  fetched the news again at every focus until a fetch succeeded (row 24). */
  let openError = $state<string | null>(null);
  function open(url: string) {
    // In words; the host logs the reason (row 24, approved; row 27).
    void openExternal(url).then((ok) => {
      openError = ok ? null : "The post could not be opened in your browser. The details are on the Logs page.";
    });
  }
</script>

<section class="home">
  <div class="bar">
    <!-- The page's heading, drawn as before (D-291). -->
    <h1>{news.view === "updates" ? "Latest updates" : "Latest from DayZ"}</h1>
    <div class="seg" role="group" aria-label="Which posts to show">
      {#each VIEWS as v (v.id)}
        <button class="segbtn" class:on={news.view === v.id} aria-pressed={news.view === v.id} title={v.title} onclick={() => (news.view = v.id)}>{v.label}</button>
      {/each}
    </div>
    {#if news.error}<span class="error" role="alert">{news.error}</span>{/if}
    {#if openError}<span class="error" role="alert">{openError}</span>{/if}
  </div>

  <div class="scroll">
    {#if featured}
      <article class="featured" class:update={featured.update} class:nopic={!hasPic(featured, true)}>
        {#if hasPic(featured, true)}
          {@const src = thumb(featured, true)}
          <!-- Out of the Tab order: its Play and Read buttons below do the same (D-291). -->
          <button class="media" tabindex="-1" onclick={() => (featured.video ? play(featured) : open(featured.url))} aria-label={featured.video ? `Play the video: ${featured.title}` : `Open the post: ${featured.title}`}>
            {#if src}<img {src} alt="" onerror={() => broken.add(featured.gid)} />{/if}
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
      <div class="grid" bind:clientWidth={gridWidth} style:grid-template-columns={layout.cols ? `repeat(${layout.cols}, minmax(0, 1fr))` : null}>
        {#each rest as n (n.gid)}
          <article class="card" class:update={n.update} use:lazy={n.gid}>
            {#if hasPic(n)}
              {@const src = cardThumb(n)}
              <!-- Named for its post: every card's picture was "Play the video" or "Open the
                   post", ahead of the card's heading (D-291). -->
              <button class="media" onclick={() => (n.video ? play(n) : open(n.url))} aria-label={n.video ? `Play the video: ${n.title}` : `Open the post: ${n.title}`}>
                {#if src}<img {src} alt="" loading="lazy" onerror={() => broken.add(n.gid)} />{/if}
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
      <!-- Nothing to call empty before the stored posts are in (row 24). -->
      <p class="muted">{news.loaded && !news.loading ? "No posts to show." : "Loading the latest posts…"}</p>
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
           allow-top-navigation is deliberately absent (D-163), and so are the popup
           tokens: the app sets no new-window handler, so a popup from the player opened
           as a plain window running YouTube outside any sandbox. "On YouTube" above
           opens the browser instead (D-283). -->
      <iframe
        src={embed(playing.id)}
        title={playing.title}
        sandbox="allow-scripts allow-same-origin allow-presentation"
        allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen"
        allowfullscreen
        referrerpolicy="strict-origin-when-cross-origin"
      ></iframe>
      <!-- Keys pressed inside YouTube's frame never reach this page, so Tab past its last
           control left the player for the page behind it; this catches it (D-291). -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <span tabindex="0" onfocus={() => closeBtn?.focus()}></span>
    </div>
  </div>
{/if}

<svelte:window onkeydown={onPlayerKey} />

<style>
  .home { display: flex; flex-direction: column; gap: 12px; min-height: 0; }

  /* Video player (D-150). */
  .player-backdrop { position: fixed; inset: 0; z-index: 60; display: grid; place-items: center; padding: 24px; background: rgb(0 0 0 / 0.72); }
  .player { width: min(1000px, 100%); display: flex; flex-direction: column; gap: 8px; }
  .player-bar { display: flex; align-items: center; gap: 8px; }
  .player-title { flex: 1; min-width: 0; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .player iframe { width: 100%; aspect-ratio: 16 / 9; max-height: 74vh; border: 0; border-radius: 12px; background: #000; box-shadow: 0 24px 60px rgb(0 0 0 / 0.5); }

  .bar { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; font-size: 12.5px; }
  .bar h1 { margin: 0 4px 0 0; font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.06em; }
  .seg { display: inline-flex; padding: 2px; border-radius: var(--radius); background: var(--bg-row); border: 1px solid var(--border-control); }
  .segbtn { all: unset; cursor: pointer; padding: 4px 11px; border-radius: 8px; color: var(--fg-muted); font-size: 12px; font-weight: 500; }
  .segbtn:hover { color: var(--fg); }
  /* `--selected-edge` is set only for the light theme's lime, amber, red and green: lime's
     fill measured 2.73:1 against the track (app.css, D-291). */
  .segbtn.on { background: var(--accent); color: var(--accent-fg); font-weight: 600; box-shadow: inset 0 0 0 1px var(--selected-edge, transparent); }
  .segbtn:focus-visible { outline: 2px solid var(--accent-ink); }
  .segbtn.on:focus-visible { outline: 2px solid var(--fg); outline-offset: 2px; }

  /* The scrollbar's room is kept whether or not it shows, so the grid's width, and the
     column count that follows it, cannot flip as the rows change the page's height (D-326). */
  .scroll { flex: 1; min-height: 0; overflow: auto; scrollbar-gutter: stable; padding-right: 4px; display: flex; flex-direction: column; gap: 12px; }
  /* The list scrolls; its children keep their natural height instead of shrinking
     (the featured card has overflow hidden, so it would otherwise collapse to 0). */
  .scroll > * { flex: none; }

  /* Pictures and video previews: a fixed 16:9 box, the image covers it; a video shows a play badge. */
  .media { all: unset; cursor: pointer; position: relative; display: block; width: 100%; aspect-ratio: 16 / 9; background: var(--bg-row); overflow: hidden; }
  .media img { width: 100%; height: 100%; object-fit: cover; display: block; transition: transform 150ms; }
  .media:hover img { transform: scale(1.03); }
  .media:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: -2px; }
  /* A dark band inside the ring, so it holds on any picture: a lime ring on foliage
     all but disappeared (D-291). */
  .media:focus-visible::after { content: ""; position: absolute; inset: 2px; border: 2px solid var(--bg); pointer-events: none; }
  .play { position: absolute; inset: 0; margin: auto; width: 56px; height: 56px; border-radius: 50%; background: rgba(0, 0, 0, 0.55); border: 2px solid rgba(255, 255, 255, 0.85); backdrop-filter: blur(2px); }
  .play::after { content: ""; position: absolute; left: 21px; top: 16px; border-style: solid; border-width: 10px 0 10px 17px; border-color: transparent transparent transparent #fff; }
  .play.small { width: 40px; height: 40px; }
  .play.small::after { left: 15px; top: 11px; border-width: 7px 0 7px 12px; }

  /* The picture's column stops at 569 px, where 16:9 meets the 320 px cap: wider, on a
     maximised window, the picture was cut top and bottom (D-326). */
  .featured { display: grid; grid-template-columns: minmax(280px, min(42%, 569px)) minmax(0, 1fr); border-radius: 14px; overflow: hidden; border: 1px solid var(--border); background: var(--bg-elev); }
  .featured.update { border-color: color-mix(in srgb, var(--accent) 50%, var(--border)); }
  /* No picture: the text takes the card's width, not the left column (row 24). */
  .featured.nopic { grid-template-columns: minmax(0, 1fr); }
  /* Capped so the hero cannot eat the window on a large screen; the grid below keeps
     more cards in view (D-143). */
  .featured { max-height: 320px; }
  /* 16:9 sets the card's height where the text is shorter; where the text is taller the
     picture stretches to it and is cut at the sides (D-326). */
  .featured .media { aspect-ratio: 16 / 9; height: auto; min-height: 200px; align-self: stretch; }
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
  /* Seven lines: with the longer summaries the featured card is filled rather than half
     empty on a wide window, and still under its 320 px cap on a narrow one (D-326). */
  .summary.feat { -webkit-line-clamp: 7; line-clamp: 7; }

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
