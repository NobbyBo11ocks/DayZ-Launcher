<script lang="ts">
  // Landing page (D-099, D-100): a welcome band with the Steam persona and avatar,
  // the newest post featured with its picture, then the latest posts as cards.
  // Pictures and video previews are thumbnails that open the post or the video in
  // the browser: no embedded players, so the memory budget holds.
  import { invoke } from "@tauri-apps/api/core";
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
  $effect(() => {
    void news.loadAvatar();
    void servers.loadHistory();
  });
  // Start-up timing for Diagnostics (D-078): the home page is the first frame now
  // (D-101); the backend keeps only the first mark it receives.
  $effect(() => {
    requestAnimationFrame(() => void invoke("perf_first_paint").catch(() => {}));
  });

  const VIEWS: { id: NewsView; label: string; title: string }[] = [
    { id: "updates", label: "Updates", title: "Game updates, hotfixes, experimental and stable releases" },
    { id: "official", label: "All news", title: "Every post from Bohemia: updates, dev blogs, sales" },
    { id: "press", label: "With press", title: "Also the press feeds Steam attaches to DayZ" },
  ];

  const hour = new Date().getHours();
  const greeting = hour < 5 ? "Still up" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
  const name = $derived(servers.steam?.persona ?? "Survivor");
  const lastJoin = $derived(servers.history[0] ?? null);
  const featured = $derived(news.list[0] ?? null);
  const rest = $derived(news.list.slice(1, 25));
  /** Chips only when there is something to say; the counts live in the title bar (D-103). */
  const showChips = $derived(servers.favourites.size > 0 || updates.state === "available");

  const day = (unix: number) => new Date(unix * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  const isNew = (n: NewsItem) => n.official && n.date > news.visitSeen;
  /** Posts whose picture failed to load: the card falls back to text (a broken image is worse than none). */
  const broken = new SvelteSet<string>();
  const thumb = (n: NewsItem, big = false) => (broken.has(n.gid) ? null : news.thumbUrl(n, big));
  const watch = (n: NewsItem) => `https://www.youtube.com/watch?v=${n.video}`;

  function open(url: string) {
    void openUrl(url).catch((e) => (news.error = String(e)));
  }
  function browse() {
    servers.navigate = "servers";
  }
  async function joinAgain() {
    if (!lastJoin) return;
    if (servers.rows.has(lastJoin.id)) {
      servers.select(lastJoin.id);
      servers.joiningId = lastJoin.id;
      return;
    }
    const row = await servers.directConnect(`${lastJoin.ip}:${lastJoin.gamePort}`);
    if (row) servers.joiningId = row.id;
  }
</script>

<section class="home">
  <header class="hero">
    <div class="who">
      {#if news.avatar}
        <img class="avatar" src={news.avatar} alt="" width="56" height="56" />
      {:else}
        <div class="avatar placeholder" aria-hidden="true">{name.slice(0, 1).toUpperCase()}</div>
      {/if}
      <div class="text">
        <h1>{greeting}, {name}</h1>
        {#if showChips}
          <p class="chips">
            {#if servers.favourites.size}<span class="chip">{servers.favourites.size} favourite{servers.favourites.size === 1 ? "" : "s"}</span>{/if}
            {#if updates.state === "available"}<span class="chip accent">Launcher {updates.version} available in Settings</span>{/if}
          </p>
        {/if}
      </div>
    </div>
    <div class="actions">
      {#if lastJoin}
        <button class="btn" onclick={joinAgain} title="{lastJoin.name} ({lastJoin.ip}:{lastJoin.gamePort})">Join again</button>
      {/if}
      <button class="btn" onclick={browse}>Browse servers</button>
    </div>
  </header>

  <div class="bar">
    <h2>{news.view === "updates" ? "Latest updates" : "Latest from DayZ"}</h2>
    <div class="seg" role="tablist" aria-label="Which posts to show">
      {#each VIEWS as v (v.id)}
        <button class="segbtn" class:on={news.view === v.id} role="tab" aria-selected={news.view === v.id} title={v.title} onclick={() => (news.view = v.id)}>{v.label}</button>
      {/each}
    </div>
    {#if news.error}<span class="error">{news.error}</span>{/if}
  </div>

  <div class="scroll">
    {#if featured}
      <article class="featured" class:update={featured.update}>
        {#if thumb(featured, true)}
          <button class="media" onclick={() => open(featured.video ? watch(featured) : featured.url)} aria-label={featured.video ? "Watch the video" : "Open the post"}>
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
          {#if featured.summary}<p class="summary">{featured.summary}</p>{/if}
          <div class="links">
            <button class="btn small" onclick={() => open(featured.url)}>Read the full post</button>
            {#if featured.video}<button class="btn small secondary" onclick={() => open(watch(featured))}>Watch on YouTube</button>{/if}
          </div>
        </div>
      </article>
      <div class="grid">
        {#each rest as n (n.gid)}
          <article class="card" class:update={n.update}>
            {#if thumb(n)}
              <button class="media" onclick={() => open(n.video ? watch(n) : n.url)} aria-label={n.video ? "Watch the video" : "Open the post"}>
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

<style>
  .home { display: flex; flex-direction: column; gap: 12px; min-height: 0; }

  /* Welcome band: accent glow from the top-left corner, persona and quick facts. */
  .hero { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 16px 20px; border-radius: 14px; border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border)); background: radial-gradient(120% 180% at 0% 0%, color-mix(in srgb, var(--accent) 24%, var(--bg-elev)) 0%, var(--bg-elev) 60%); box-shadow: 0 12px 32px rgba(0, 0, 0, 0.25); }
  .who { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .avatar { width: 56px; height: 56px; border-radius: 50%; border: 2px solid var(--accent); box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 20%, transparent); flex: none; }
  .avatar.placeholder { display: grid; place-items: center; background: var(--bg-row); color: var(--accent); font-weight: 700; font-size: 22px; }
  .text { min-width: 0; }
  .text h1 { margin: 0; font-size: 23px; font-weight: 650; letter-spacing: -0.01em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; margin: 6px 0 0; }
  .chip { padding: 2px 9px; border-radius: 999px; background: color-mix(in srgb, var(--fg) 7%, transparent); border: 1px solid var(--border); font-size: 11.5px; color: var(--fg-muted); white-space: nowrap; }
  .chip.accent { color: #111; background: var(--accent); border-color: transparent; font-weight: 600; }
  .actions { display: flex; gap: 8px; flex: none; flex-wrap: wrap; justify-content: flex-end; }

  .btn { all: unset; cursor: pointer; padding: 8px 14px; border-radius: 10px; background: var(--accent); color: #111; font-weight: 600; white-space: nowrap; max-width: 320px; overflow: hidden; text-overflow: ellipsis; }
  .btn.secondary { background: var(--bg-row); color: var(--fg); border: 1px solid var(--border); font-weight: 500; }
  .btn.small { padding: 4px 10px; font-size: 12px; border-radius: var(--radius); }
  .btn:hover { filter: brightness(1.08); }
  .btn.secondary:hover { border-color: var(--accent); filter: none; }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .btn:focus-visible { outline: 2px solid var(--fg); }

  .bar { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; font-size: 12.5px; }
  .bar h2 { margin: 0 4px 0 0; font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.06em; }
  .seg { display: inline-flex; padding: 2px; border-radius: 10px; background: var(--bg-row); border: 1px solid var(--border); }
  .segbtn { all: unset; cursor: pointer; padding: 4px 11px; border-radius: 8px; color: var(--fg-muted); font-size: 12px; font-weight: 500; }
  .segbtn:hover { color: var(--fg); }
  .segbtn.on { background: var(--accent); color: #111; font-weight: 600; }
  .segbtn:focus-visible { outline: 2px solid var(--accent); }

  .scroll { flex: 1; min-height: 0; overflow: auto; padding-right: 4px; display: flex; flex-direction: column; gap: 12px; }
  /* The list scrolls; its children keep their natural height instead of shrinking
     (the featured card has overflow hidden, so it would otherwise collapse to 0). */
  .scroll > * { flex: none; }

  /* Pictures and video previews: a fixed 16:9 box, the image covers it; a video shows a play badge. */
  .media { all: unset; cursor: pointer; position: relative; display: block; width: 100%; aspect-ratio: 16 / 9; background: var(--bg-row); overflow: hidden; }
  .media img { width: 100%; height: 100%; object-fit: cover; display: block; transition: transform 150ms; }
  .media:hover img { transform: scale(1.03); }
  .media:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .play { position: absolute; inset: 0; margin: auto; width: 56px; height: 56px; border-radius: 50%; background: rgba(0, 0, 0, 0.55); border: 2px solid rgba(255, 255, 255, 0.85); backdrop-filter: blur(2px); }
  .play::after { content: ""; position: absolute; left: 21px; top: 16px; border-style: solid; border-width: 10px 0 10px 17px; border-color: transparent transparent transparent #fff; }
  .play.small { width: 40px; height: 40px; }
  .play.small::after { left: 15px; top: 11px; border-width: 7px 0 7px 12px; }

  .featured { display: grid; grid-template-columns: minmax(280px, 42%) minmax(0, 1fr); border-radius: 14px; overflow: hidden; border: 1px solid var(--border); background: var(--bg-elev); }
  .featured.update { border-color: color-mix(in srgb, var(--accent) 50%, var(--border)); }
  .featured .media { aspect-ratio: auto; height: 100%; min-height: 220px; }
  .featured .body { padding: 18px 20px; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .featured h3 { margin: 0; font-size: 19px; font-weight: 650; line-height: 1.25; }
  .links { display: flex; gap: 8px; margin-top: auto; padding-top: 4px; flex-wrap: wrap; }

  .meta { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; font-size: 11.5px; color: var(--fg-muted); }
  .dot { opacity: 0.6; }
  .pill { padding: 1px 7px; border-radius: 9px; font-size: 10.5px; font-weight: 600; letter-spacing: 0.02em; }
  .pill.up { background: color-mix(in srgb, var(--accent) 22%, var(--bg-row)); color: var(--accent); }
  .pill.new { background: var(--accent); color: #111; }
  .link { all: unset; cursor: pointer; color: var(--fg); }
  .link:hover { color: var(--accent); }
  .link:focus-visible { outline: 2px solid var(--accent); border-radius: 4px; }
  .summary { margin: 0; font-size: 13px; color: var(--fg-muted); line-height: 1.5; overflow-wrap: anywhere; }
  .summary.clamp { display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }

  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; padding-bottom: 4px; }
  .card { display: flex; flex-direction: column; border-radius: 12px; overflow: hidden; background: var(--bg-elev); border: 1px solid var(--border); transition: transform 150ms, border-color 150ms; min-width: 0; }
  .card:hover { transform: translateY(-2px); border-color: color-mix(in srgb, var(--accent) 45%, var(--border)); }
  .card.update { box-shadow: inset 3px 0 0 var(--accent); }
  .card .body { padding: 12px 14px 14px; display: flex; flex-direction: column; gap: 6px; }
  .card h3 { margin: 0; font-size: 14px; font-weight: 600; line-height: 1.3; }
  .error { color: var(--danger); }

  @media (max-width: 1100px) {
    .featured { grid-template-columns: 1fr; }
    .featured .media { aspect-ratio: 16 / 9; height: auto; min-height: 0; }
    .hero { flex-direction: column; align-items: flex-start; }
    .actions { justify-content: flex-start; }
  }
</style>
