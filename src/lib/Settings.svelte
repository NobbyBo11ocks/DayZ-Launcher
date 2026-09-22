<script lang="ts">
  // Appearance, browser defaults, launch options and profiles, updates, Steam
  // (docs/06 §5). Two columns so the whole view fits the viewport (D-094).
  import { invoke } from "@tauri-apps/api/core";
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

  const themes: { id: Theme; label: string }[] = [
    { id: "slate", label: "Slate (dark)" },
    { id: "light", label: "Light" },
  ];
  // Twelve accents as colour dots (D-131): the id is the label, capitalised.
  const accents = ACCENTS.map((id) => ({ id, label: id.charAt(0).toUpperCase() + id.slice(1) }));
</script>

<section class="settings">
  <h1>Settings</h1>

  <div class="cols">
    <div class="col">
      <h2>Appearance</h2>
      <div class="row">
        <span class="label">Theme</span>
        <div class="group" role="radiogroup" aria-label="Theme">
          {#each themes as t (t.id)}
            <button class="chip" class:on={prefs.theme === t.id} role="radio" aria-checked={prefs.theme === t.id} onclick={() => (prefs.theme = t.id)}>{t.label}</button>
          {/each}
        </div>
      </div>
      <div class="row">
        <span class="label">Accent</span>
        <div class="group dots" role="radiogroup" aria-label="Accent">
          {#each accents as a (a.id)}
            <button class="dot" data-accent={a.id} class:on={prefs.accent === a.id} role="radio" aria-checked={prefs.accent === a.id} aria-label={a.label} title={a.label} onclick={() => (prefs.accent = a.id)}></button>
          {/each}
        </div>
        <span class="muted">{accents.find((a) => a.id === prefs.accent)?.label}</span>
      </div>

      <h2>Server browser</h2>
      <label class="row check">
        <input type="checkbox" bind:checked={servers.filters.hideUntrusted} onchange={() => servers.saveFilters()} />
        <span>Hide servers with inflated or unverifiable player counts by default</span>
      </label>
      <p class="muted note">
        Player counts are verified directly with each server (A2S_PLAYER) and cross-checked against Steam's authenticated player data. See docs/11 for the rules.
      </p>

      <h2>Updates</h2>
      <div class="row">
        <span class="label">Version</span>
        <span>{app ? `${app.version} · Tauri ${app.tauri}` : "…"}</span>
      </div>
      <div class="row wrap">
        <button class="chip" onclick={() => updates.checkNow()} disabled={updates.state === "checking" || updates.state === "downloading"}>
          {updates.state === "checking" ? "Checking…" : "Check for updates"}
        </button>
        {#if updates.state === "none"}<span class="ok">Up to date.</span>{/if}
        {#if updates.state === "available"}
          <span>Version {updates.version} is available.</span>
          <button class="chip on" onclick={() => updates.install()}>Install and restart</button>
        {/if}
        {#if updates.state === "downloading"}<span class="muted">Downloading… {updates.progress}%</span>{/if}
        {#if updates.state === "ready"}<span class="ok">Installed, restarting…</span>{/if}
        {#if updates.state === "error"}<span class="error">Update check failed: {updates.error}</span>{/if}
      </div>
      <p class="muted note">Updates are signed; the launcher only installs packages whose signature matches its built-in public key.</p>

      <h2>Steam</h2>
      <dl class="kv">
        <dt>Status</dt>
        <dd>
          {servers.steam?.initialized ? `connected as ${servers.steam.persona ?? "?"}` : (servers.steam?.error ?? "not connected")}
          {#if servers.steam?.idle}<span class="muted"> · released while idle (reconnects on use)</span>{/if}
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
            after <input class="text num" type="number" min="0" max="1440" step="1" bind:value={launch.steamIdleMinutes} onchange={scheduleSave} aria-label="Minutes before the Steam session is released" /> min
            <span class="muted">(0 = keep connected)</span>
          </span>
        </label>
        <p class="muted note">
          While connected, Steam shows you as playing DayZ and counts playtime, like the official launcher. Releasing the session when idle stops that; it reconnects by itself for the next refresh, join or friends lookup.
        </p>
      {/if}

      <h2>Credits</h2>
      <p class="muted note">
        IP geolocation by <a href="https://db-ip.com" onclick={external}>DB-IP</a> (IP to Country Lite, CC BY 4.0).
        Flags by <a href="https://github.com/lipis/flag-icons" onclick={external}>flag-icons</a> (MIT).
        Server data from Steam and the servers themselves; nothing is sent anywhere else.
      </p>
    </div>

    <div class="col">
      <h2>Launch</h2>
      {#if launch}
        <label class="row">
          <span class="label">Profile name</span>
          <input class="text" type="text" bind:value={launch.profileName} oninput={scheduleSave} placeholder={servers.steam?.persona ?? "Steam persona"} aria-label="In-game profile name" />
        </label>
        <label class="row check"><input type="checkbox" bind:checked={launch.skipIntro} onchange={scheduleSave} /> Skip intro (<code>-skipintro</code>)</label>
        <label class="row check"><input type="checkbox" bind:checked={launch.noSplash} onchange={scheduleSave} /> No splash screen (<code>-nosplash</code>)</label>
        <label class="row check"><input type="checkbox" bind:checked={launch.noPause} onchange={scheduleSave} /> Keep running when unfocused (<code>-noPause</code>)</label>
        <label class="row">
          <span class="label">Extra args</span>
          <input class="text mono" type="text" bind:value={launch.extraArgs} oninput={scheduleSave} placeholder="-cpuCount=8 -profiles=&quot;D:\Profiles&quot;" aria-label="Extra launch arguments" />
        </label>
        <p class="muted note">Passed to DayZ after the server, mods and profile arguments (docs/02 §5). {#if saved}<span class="ok">Saved.</span>{/if}{#if saveError}<span class="error">{saveError}</span>{/if}</p>

        <h2>Launch profiles</h2>
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
        <p class="muted note">Profiles are presets of the launch options above; the join dialog can start a server with any of them.</p>
      {:else}
        <p class="muted note">Loading…</p>
      {/if}
    </div>
  </div>
</section>

<style>
  .settings { display: flex; flex-direction: column; min-height: 0; }
  h1 { font-size: 18px; font-weight: 600; margin: 0 0 6px; }
  /* Two equal columns; the grid scrolls only when the window is smaller than the content. */
  .cols { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 0 32px; overflow: auto; }
  .col { display: flex; flex-direction: column; gap: 7px; min-width: 0; }
  h2 { font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; margin: 12px 0 1px; }
  .col > h2:first-child { margin-top: 0; }
  .row { display: flex; align-items: center; gap: 12px; }
  .row.wrap { flex-wrap: wrap; }
  .label { flex: 0 0 82px; color: var(--fg-muted); }
  .group { display: inline-flex; gap: 6px; flex-wrap: wrap; }
  .chip { all: unset; cursor: pointer; padding: 5px 11px; border-radius: var(--radius); border: 1px solid var(--border); color: var(--fg-muted); display: inline-flex; align-items: center; gap: 8px; }
  .chip.on { background: color-mix(in srgb, var(--accent) 22%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  .chip:disabled { opacity: 0.5; cursor: default; }
  .chip:focus-visible { outline: 2px solid var(--accent); }
  /* Accent dots: each carries its own --accent through its data-accent attribute (app.css), so the
     fill and the selection ring are the dot's colour, not the current theme's. */
  .dots { gap: 8px; align-items: center; }
  .dot { all: unset; box-sizing: border-box; cursor: pointer; width: 18px; height: 18px; border-radius: 50%; background: var(--accent); box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.25); transition: transform 120ms ease; }
  .dot:hover { transform: scale(1.15); }
  .dot.on { box-shadow: 0 0 0 2px var(--bg), 0 0 0 4px var(--accent); }
  .dot:focus-visible { outline: 2px solid var(--fg); outline-offset: 3px; }
  .check { cursor: pointer; }
  .check input { accent-color: var(--accent); }
  .text { flex: 1; min-width: 0; max-width: 420px; padding: 5px 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); }
  .inline { display: inline-flex; align-items: center; gap: 6px; min-width: 0; }
  .inline.wrap { flex-wrap: wrap; }
  .inline .text { flex: 0 1 auto; width: auto; }
  .text.num { width: 64px; flex: none; }
  .text:focus-visible { outline: 2px solid var(--accent); }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; color: var(--fg-muted); }
  .ok { color: var(--ok); }
  .note { font-size: 12px; margin: 0; }
  .kv { display: grid; grid-template-columns: max-content 1fr; gap: 3px 16px; margin: 0; }
  .kv dt { color: var(--fg-muted); }
  .kv dd { margin: 0; }
</style>
