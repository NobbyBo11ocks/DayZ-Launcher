<script lang="ts">
  // Appearance and browser defaults (docs/06 §5). Launch options arrive in M5.
  import { invoke } from "@tauri-apps/api/core";
  import { prefs, type Accent, type Theme } from "./state/prefs.svelte";
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
  const accents: { id: Accent; label: string }[] = [
    { id: "amber", label: "Amber" },
    { id: "teal", label: "Teal" },
    { id: "red", label: "Red" },
    { id: "green", label: "Green" },
  ];
</script>

<section class="settings">
  <h1>Settings</h1>

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
    <div class="group" role="radiogroup" aria-label="Accent">
      {#each accents as a (a.id)}
        <button class="chip" class:on={prefs.accent === a.id} role="radio" aria-checked={prefs.accent === a.id} onclick={() => (prefs.accent = a.id)}>
          <span class="swatch" data-accent={a.id}></span>{a.label}
        </button>
      {/each}
    </div>
  </div>

  <h2>Server browser</h2>
  <label class="row check">
    <input type="checkbox" bind:checked={servers.filters.hideUntrusted} onchange={() => servers.saveFilters()} />
    <span>Hide servers with inflated or unverifiable player counts by default</span>
  </label>
  <p class="muted note">
    Player counts are verified directly with each server (A2S_PLAYER) and cross-checked against Steam's authenticated
    player data. See docs/11 for the rules.
  </p>

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
  {/if}

  <h2>Updates</h2>
  <div class="row">
    <span class="label">Version</span>
    <span>{app ? `${app.version} · Tauri ${app.tauri}` : "…"}</span>
  </div>
  <div class="row">
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
</section>

<style>
  .settings { display: flex; flex-direction: column; gap: 10px; max-width: 640px; }
  h1 { font-size: 18px; font-weight: 600; margin: 0 0 4px; }
  h2 { font-size: 12px; font-weight: 600; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; margin: 14px 0 2px; }
  .row { display: flex; align-items: center; gap: 12px; }
  .label { width: 70px; color: var(--fg-muted); }
  .group { display: inline-flex; gap: 6px; flex-wrap: wrap; }
  .chip { all: unset; cursor: pointer; padding: 6px 12px; border-radius: var(--radius); border: 1px solid var(--border); color: var(--fg-muted); display: inline-flex; align-items: center; gap: 8px; }
  .chip.on { background: color-mix(in srgb, var(--accent) 22%, var(--bg-row)); color: var(--fg); border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  .chip:focus-visible { outline: 2px solid var(--accent); }
  .swatch { width: 12px; height: 12px; border-radius: 50%; display: inline-block; }
  .swatch[data-accent="amber"] { background: #f0b429; }
  .swatch[data-accent="teal"] { background: #2dd4bf; }
  .swatch[data-accent="red"] { background: #f0544f; }
  .swatch[data-accent="green"] { background: #4ade80; }
  .check { cursor: pointer; }
  .check input { accent-color: var(--accent); }
  .text { flex: 1; max-width: 420px; padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--bg-row); color: var(--fg); }
  .text:focus-visible { outline: 2px solid var(--accent); }
  .mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; }
  code { font-family: Consolas, "Cascadia Mono", monospace; font-size: 11.5px; color: var(--fg-muted); }
  .ok { color: var(--ok); }
  .note { font-size: 12px; margin: 0; }
  .kv { display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; margin: 0; }
  .kv dt { color: var(--fg-muted); }
  .kv dd { margin: 0; }
</style>
