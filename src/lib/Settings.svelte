<script lang="ts">
  // Appearance, browser and news defaults, launch options and profiles, Steam and
  // updates (docs/06 §5), as cards in two columns so the whole view fits the viewport
  // without page scrolling (D-094, D-180).
  // Every command through the logging wrapper: a failure is recorded with its
  // command name before it is rethrown (D-158).
  import { invokeLogged as invoke } from "./log";
  import { external } from "./external";
  import { ACCENTS, prefs, type Theme } from "./state/prefs.svelte";
  import { servers } from "./state/servers.svelte";
  import { updates } from "./state/updates.svelte";
  import type { Settings } from "./types";

  type AppInfo = { name: string; version: string; tauri: string; os: string };
  let app = $state<AppInfo | null>(null);
  $effect(() => {
    invoke<AppInfo>("app_info")
      .then((i) => (app = i))
      .catch(() => {});
  });

  let launch = $state<Settings | null>(null);
  let saved = $state(false);
  let saveError = $state<string | null>(null);

  $effect(() => {
    invoke<Settings>("settings_get")
      .then((s) => (launch = s))
      .catch((e) => (saveError = String(e)));
  });

  // Launch profiles (D-088): presets of the flat launch options.
  let pick = $state("");
  let newName = $state("");
  function saveProfile() {
    if (!launch) return;
    const name = newName.trim();
    if (!name) return;
    const p = { name, profileName: launch.profileName, extraArgs: launch.extraArgs, skipIntro: launch.skipIntro, noSplash: launch.noSplash, noPause: launch.noPause };
    launch.launchProfiles = [...launch.launchProfiles.filter((x) => x.name !== name), p];
    newName = "";
    pick = name;
    scheduleSave();
  }
  function loadProfile() {
    const p = launch?.launchProfiles.find((x) => x.name === pick);
    if (!launch || !p) return;
    launch.profileName = p.profileName;
    launch.extraArgs = p.extraArgs;
    launch.skipIntro = p.skipIntro;
    launch.noSplash = p.noSplash;
    launch.noPause = p.noPause;
    scheduleSave();
  }
  function deleteProfile() {
    if (!launch || !pick) return;
    launch.launchProfiles = launch.launchProfiles.filter((x) => x.name !== pick);
    pick = "";
    scheduleSave();
  }

  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleSave() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      if (!launch) return;
      try {
        await invoke("settings_set", { settings: launch });
        saved = true;
        setTimeout(() => (saved = false), 1200);
      } catch (e) {
        saveError = String(e);
      }
    }, 300);
  }

  // An emptied number field binds as null and Rust's u32 refuses it, so the save
  // throws — and because the null stays in `launch`, every later save throws too and
  // the whole page silently stops persisting. Clamp before scheduling (D-209).
  function saveIdle() {
    if (!launch) return;
    const n = Math.round(Number(launch.steamIdleMinutes));
    launch.steamIdleMinutes = Number.isFinite(n) ? Math.min(1440, Math.max(0, n)) : 15;
    scheduleSave();
  }

  const themes: { id: Theme; label: string }[] = [
    { id: "slate", label: "Slate (dark)" },
    { id: "light", label: "Light" },
  ];
  // Twelve accents as colour dots (D-131): the id is the label, capitalised.
  const accents = ACCENTS.map((id) => ({ id, label: id.charAt(0).toUpperCase() + id.slice(1) }));

  /**
   * Arrow keys inside a radiogroup, which is what makes it one tab stop instead of
   * one per option — twelve of them for the accents alone (D-198).
   */
  function roving(e: KeyboardEvent, ids: string[], current: string, pick: (id: string) => void) {
    const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    e.preventDefault();
    const i = ids.indexOf(current);
    const next = ids[(i + step + ids.length) % ids.length];
    if (!next) return;
    pick(next);
    // The newly checked option is the one that now holds the tab stop, so focus follows.
    const group = e.currentTarget as HTMLElement;
    queueMicrotask(() => group.querySelector<HTMLElement>('[tabindex="0"]')?.focus());
  }
</script>

<section class="settings">
  <header class="head">
    <div class="title">
      <h1>Settings</h1>
      <span class="sub">Appearance, what the browser shows, how DayZ is started, and the launcher itself</span>

    </div>
    <div class="state" role="status" aria-live="polite">
      {#if saved}<span class="ok">Saved</span>{/if}
      {#if saveError}<span class="error">{saveError}</span>{/if}
    </div>
  </header>

  <div class="cols">
    <div class="col">
      <section class="card">
        <h2>Appearance</h2>
        <div class="cbody">
          <div class="row">
            <span class="label">Theme</span>
            <div class="group" role="radiogroup" aria-label="Theme" tabindex="-1" onkeydown={(e) => roving(e, themes.map((t) => t.id), prefs.theme, (id) => (prefs.theme = id as typeof prefs.theme))}>
              {#each themes as t (t.id)}
                <button class="chip" class:on={prefs.theme === t.id} role="radio" aria-checked={prefs.theme === t.id} tabindex={prefs.theme === t.id ? 0 : -1} onclick={() => (prefs.theme = t.id)}>{t.label}</button>
              {/each}
            </div>
          </div>
          <div class="row">
            <span class="label">Accent</span>
            <div class="group dots" role="radiogroup" aria-label="Accent" tabindex="-1" onkeydown={(e) => roving(e, accents.map((a) => a.id), prefs.accent, (id) => (prefs.accent = id as typeof prefs.accent))}>
              {#each accents as a (a.id)}
                <button class="dot" data-accent={a.id} class:on={prefs.accent === a.id} role="radio" aria-checked={prefs.accent === a.id} aria-label={a.label} title={a.label} tabindex={prefs.accent === a.id ? 0 : -1} onclick={() => (prefs.accent = a.id)}></button>
              {/each}
              <span class="muted swatch">{accents.find((a) => a.id === prefs.accent)?.label}</span>
            </div>
          </div>
        </div>
      </section>

      <section class="card">
        <h2>What the launcher shows</h2>
        <div class="cbody">
          <label class="check">
            <input type="checkbox" bind:checked={servers.filters.hideUntrusted} onchange={() => servers.saveFilters()} />
            <span>Hide servers with inflated or unverifiable player counts</span>
          </label>
          <!-- Off removes the page and stops the feed being fetched at all (D-174). -->
          <label class="check">
            <input type="checkbox" checked={prefs.news} onchange={(e) => prefs.setNews(e.currentTarget.checked)} />
            <span>Show DayZ news on a page of its own</span>
          </label>
          <p class="note">Off means the launcher never contacts Steam's news feed, its picture CDN or YouTube.</p>
        </div>
      </section>

      <section class="card">
        <h2>Steam</h2>
        <div class="cbody">
          <dl class="kv">
            <dt>Status</dt>
            <dd class={servers.steam?.initialized ? "ok" : "warn"}>
              {servers.steam?.initialized ? `connected as ${servers.steam.persona ?? "?"}` : (servers.steam?.error ?? "not connected")}
              {#if servers.steam?.idle}<span class="muted"> · released while idle, reconnects on use</span>{/if}
            </dd>
            <dt>Local DayZ</dt>
            <dd>{servers.localVersion ?? "not found"}</dd>
            <dt>Last refresh</dt>
            <dd>{servers.lastRefresh ? new Date(servers.lastRefresh * 1000).toLocaleString() : "never"}</dd>
          </dl>
          {#if launch}
            <label class="row">
              <span class="label">Idle release</span>
              <span class="inline">
                after <input class="text num" type="number" min="0" max="1440" step="1" bind:value={launch.steamIdleMinutes} onchange={saveIdle} aria-label="Minutes before the Steam session is released" /> min
                <span class="muted">0 = stay connected</span>
              </span>
            </label>
            <p class="note">Steam counts playtime while connected. Releasing stops that; it reconnects by itself when needed.</p>
          {/if}
        </div>
      </section>

      <section class="card tail">
        <h2>The launcher itself</h2>
        <div class="cbody">
          <div class="row">
            <span class="label">Version</span>
            <span>{app ? `${app.version} · Tauri ${app.tauri}` : "…"}</span>
          </div>
          <div class="row wrap">
            <span class="label"></span>
            <span class="inline wrap" role="status" aria-live="polite">
              <button class="chip accent" onclick={() => updates.checkNow()} disabled={updates.state === "checking" || updates.state === "downloading"}>
                {updates.state === "checking" ? "Checking…" : "Check for updates"}
              </button>
              {#if updates.state === "none"}<span class="ok">Up to date.</span>{/if}
              {#if updates.state === "available"}
                <span>Version {updates.version} is available.</span>
                <button class="chip accent" onclick={() => updates.install()}>Install and restart</button>
              {/if}
              {#if updates.state === "downloading"}<span class="muted">Downloading… {updates.progress}%</span>{/if}
              {#if updates.state === "ready"}<span class="ok">Installed, restarting…</span>{/if}
              {#if updates.error}<span class="error" role="alert">{updates.error}</span>{/if}
            </span>
          </div>
          <p class="note">Updates install only when their signature matches this build's key.</p>
          <p class="note credits">
            IP geolocation by <a href="https://db-ip.com" onclick={external}>DB-IP</a> (IP to Country Lite, CC BY 4.0).
            Flags by <a href="https://github.com/lipis/flag-icons" onclick={external}>flag-icons</a> (MIT).
            Server data comes from Steam and the servers themselves; nothing is sent anywhere else.
          </p>
        </div>
      </section>
    </div>

    <div class="col">
      <section class="card">
        <h2>How DayZ starts</h2>
        <div class="cbody">
          {#if launch}
            <label class="row">
              <span class="label">Profile name</span>
              <input class="text" type="text" bind:value={launch.profileName} oninput={scheduleSave} placeholder={servers.steam?.persona ?? "Steam persona"} aria-label="In-game profile name" />
            </label>
            <label class="check"><input type="checkbox" bind:checked={launch.skipIntro} onchange={scheduleSave} /> <span>Skip intro <code>-skipintro</code></span></label>
            <label class="check"><input type="checkbox" bind:checked={launch.noSplash} onchange={scheduleSave} /> <span>No splash screen <code>-nosplash</code></span></label>
            <label class="check"><input type="checkbox" bind:checked={launch.noPause} onchange={scheduleSave} /> <span>Keep running when unfocused <code>-noPause</code></span></label>
            <label class="row">
              <span class="label">Extra args</span>
              <input class="text mono" type="text" bind:value={launch.extraArgs} oninput={scheduleSave} placeholder="-cpuCount=8 -profiles=&quot;D:\Profiles&quot;" aria-label="Extra launch arguments" />
            </label>

            <h3>Saved profiles</h3>
            <div class="row">
              <span class="label">Saved</span>
              <span class="inline wrap">
                <select class="text" bind:value={pick} aria-label="Saved launch profiles">
                  <option value="">Saved profiles…</option>
                  {#each launch.launchProfiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
                </select>
                <button class="chip" onclick={loadProfile} disabled={!pick} title="Copy this profile into the launch options above">Load</button>
                <button class="chip" onclick={deleteProfile} disabled={!pick}>Delete</button>
              </span>
            </div>
            <div class="row">
              <span class="label">New</span>
              <span class="inline wrap">
                <input class="text" type="text" placeholder="Profile name" bind:value={newName} aria-label="New profile name" />
                <button class="chip" onclick={saveProfile} disabled={!newName.trim()} title="Save the launch options above under this name">Save as profile</button>
              </span>
            </div>
          {:else}
            <p class="note">Loading…</p>
          {/if}
        </div>
      </section>

    </div>
  </div>
</section>

<style>
  /* Cards in two columns (D-180), the same shape as the Logs page: a header bar per
     group, one label column so every control lines up, and each note under the control
     it explains. The grid scrolls only when the window is smaller than the content
     (D-094). */
  .settings { display: flex; flex-direction: column; gap: 10px; min-height: 0; }

  .head { display: flex; align-items: flex-end; gap: 16px; }
  .title { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  h1 { margin: 0; font-size: 19px; line-height: 1.1; }
  .sub { color: var(--fg-muted); font-size: 12px; }
  .state { margin-left: auto; font-size: 12px; }

  .cols { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 10px; overflow: auto; align-content: stretch; }
  .col { display: flex; flex-direction: column; gap: 10px; min-width: 0; }

  /* No panels. The page is a list of settings, not a set of boxes — the borders,
     fills and header strips were most of what it looked like and none of them carried
     meaning. A heading and the space under it separate the groups; the only thing that
     still looks like a control is a control (D-203). */
  .card { border: 0; background: none; }
  .card + .card { margin-top: 4px; }
  /* The update check sits at the very foot of the left column, away from the settings
     above it: it is the one thing on this page that acts rather than configures, and
     flush under the Steam card it read as a fifth setting (D-217, D-218). `auto`, so
     it stays on the bottom edge at any window height; with no free space to take it
     resolves to 0 and the card simply follows the one above. Ordered after the sibling
     rule above, which has the same specificity. */
  .card.tail { margin-top: auto; }
  h2 { display: flex; align-items: center; margin: 0; font-size: 10.5px; font-weight: 600; color: var(--accent-ink); text-transform: uppercase; letter-spacing: 0.07em; }
  /* A second heading inside a group, for the one that has two halves. */
  h3 { margin: 8px 0 0; font-size: 10.5px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.07em; }
  .cbody { display: flex; flex-direction: column; gap: 8px; padding: 6px 0 16px; }
  .credits { margin-top: 2px; padding-top: 8px; border-top: 1px solid var(--border); }

  .row { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .row.wrap { flex-wrap: wrap; }
  .label { flex: 0 0 92px; color: var(--fg-muted); font-size: 12.5px; }
  .group { display: inline-flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .swatch { margin-left: 4px; font-size: 12px; }

  .chip { all: unset; cursor: pointer; padding: 5px 11px; border-radius: var(--radius); border: 1px solid var(--border); color: var(--fg-muted); font-size: 12.5px; display: inline-flex; align-items: center; gap: 8px; white-space: nowrap; }
  .chip:hover { border-color: var(--accent); color: var(--fg); }
  .chip.on { background: color-mix(in srgb, var(--accent) 22%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  /* The primary action of its card, so it wears the accent (user request, D-180). */
  .chip.accent { background: var(--accent); color: var(--accent-fg); border-color: transparent; font-weight: 600; }
  .chip.accent:hover { filter: brightness(1.08); color: var(--accent-fg); }
  .chip:disabled { opacity: 0.5; cursor: default; }
  .chip:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 1px; }

  /* Accent dots: each carries its own --accent through its data-accent attribute (app.css), so the
     fill and the selection ring are the dot's colour, not the current theme's. */
  .dots { gap: 8px; }
  .dot { all: unset; box-sizing: border-box; cursor: pointer; width: 18px; height: 18px; border-radius: 50%; background: var(--accent); box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.25); transition: transform 120ms ease; }
  .dot:hover { transform: scale(1.15); }
  .dot.on { box-shadow: 0 0 0 2px var(--bg-elev), 0 0 0 4px var(--accent); }
  .dot:focus-visible { outline: 2px solid var(--fg); outline-offset: 3px; }

  .check { display: flex; align-items: center; gap: 9px; cursor: pointer; font-size: 12.5px; }
  .check input { accent-color: var(--accent); flex: none; }

  .text { flex: 1; min-width: 0; max-width: 420px; padding: 5px 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); font-size: 12.5px; }
  .text:focus-visible { outline: 2px solid var(--accent-ink); }
  .text.num { width: 62px; flex: none; }
  .inline { display: inline-flex; align-items: center; gap: 6px; min-width: 0; font-size: 12.5px; }
  .inline.wrap { flex-wrap: wrap; }
  .inline .text { flex: 0 1 auto; width: auto; }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; color: var(--fg-muted); }

  .kv { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 3px 16px; margin: 0; font-size: 12.5px; }
  .kv dt { color: var(--fg-muted); }
  .kv dd { margin: 0; min-width: 0; }

  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .note { margin: 0; font-size: 11.5px; line-height: 1.5; color: var(--fg-muted); }
</style>
