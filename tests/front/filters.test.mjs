// Saved filters and the search box: `sanitizeFilters` and the store's start-up
// reconciliation (servers.svelte.ts), `searchBox` (search.ts) and the map names the search
// matches (maps.ts).
import test from "node:test";
import assert from "node:assert/strict";
import { advance, deferred, freshApp, row, settle } from "./harness.mjs";

const FILTERS_KEY = "dayz-launcher.filters.v1";

function settingsWith(ui) {
  return {
    profileName: "",
    extraArgs: "",
    skipIntro: true,
    noSplash: true,
    noPause: false,
    launchProfiles: [],
    steamIdleMinutes: 5,
    logging: true,
    logMuted: [],
    ui: { theme: "slate", accent: "lime", onboarded: true, filters: null, lastUpdateCheckMs: 0, newsSeen: 0, news: true, lastSeenVersion: "", openOn: "news", ...ui },
  };
}

test("a saved filter set is taken key by key: a bad value falls back alone, unknown keys are kept, nothing throws (D-304 (8), row 15 F8)", async (t) => {
  const app = await freshApp(t, { storage: { [FILTERS_KEY]: JSON.stringify({ perspective: "1pp", maxPing: "abc", map: "DeerIsle", mods: "sideways" }) } });
  const file = deferred();
  app.mock.handle("settings_get", () => file.promise);
  const { servers, sanitizeFilters, defaultFilters } = await app.load("state/servers.svelte");

  // The first frame, from the page's own copy.
  assert.equal(servers.filters.perspective, "1pp");
  assert.equal(servers.filters.maxPing, 0);
  assert.equal(servers.filters.map, "deerisle", "saved before D-195 in the server's capitalisation");
  assert.equal(servers.filters.mods, "any");

  // Then the settings file, hand-edited or written by another version.
  file.resolve(settingsWith({ filters: { map: 42, mods: "modded", maxPing: "150", hideUntrusted: "no", mod: "1559212036", futureKey: { a: 1 } } }));
  await settle();
  assert.equal(servers.filters.mods, "modded");
  assert.equal(servers.filters.maxPing, 150, "a ping limit in digits");
  assert.equal(servers.filters.mod, 1559212036, "a Workshop id in digits");
  assert.equal(servers.filters.map, "", "a map that is not text");
  assert.equal(servers.filters.hideUntrusted, true, "a switch that is not true or false keeps its default");
  assert.deepEqual(servers.filters.futureKey, { a: 1 }, "a key this build does not know is kept for the one that wrote it");
  assert.equal(servers.activeFilterCount, 3, "Reset counts what filters: modded, ping, the mod");

  const d = defaultFilters();
  for (const junk of [null, undefined, 7, "x", [1, 2], true]) assert.deepEqual(sanitizeFilters(junk), d);
  const f = sanitizeFilters({ perspective: "2pp", country: 7, mod: 1.5, maxPing: -5, notFull: "yes", hive: "official", style: "pve", search: "never saved" });
  assert.equal(f.perspective, "any");
  assert.equal(f.country, "");
  assert.equal(f.mod, 0);
  assert.equal(f.maxPing, 0);
  assert.equal(f.notFull, false);
  assert.equal(f.hive, "official");
  assert.equal(f.style, "pve");
  assert.equal(f.search, "");
  assert.equal(sanitizeFilters({ mod: "12a" }).mod, 0);
});

test("typing a search before the settings file arrives does not cost the saved filters; clearing it applies at once; maps are found by their names (D-304 (7) F4, D-222, D-195)", async (t) => {
  const app = await freshApp(t);
  const file = deferred();
  app.mock.handle("settings_get", () => file.promise);
  const { servers } = await app.load("state/servers.svelte");
  const { searchBox } = await app.load("search");
  const { mapLabel } = await app.load("maps");
  await servers.start();
  const livonia = row({ name: "Alpha", map: "enoch" });
  const sakhal = row({ name: "Bravo", map: "Sakhal" });
  const cherno = row({ name: "Charlie", map: "ChernarusPlus" });
  app.mock.send({ kind: "batch", data: [livonia, sakhal, cherno] });
  servers.flushRows();

  const box = searchBox();
  box.set("livonia");
  await advance(t, 100);
  assert.equal(servers.filters.search, "", "debounced");
  await advance(t, 100);
  assert.deepEqual(servers.list.map((r) => r.id), [livonia.id], "Livonia is enoch");
  box.set("frostline");
  await advance(t, 200);
  assert.deepEqual(servers.list.map((r) => r.id), [sakhal.id], "the DLC's name finds Sakhal");
  box.set("chernarus");
  await advance(t, 200);
  assert.deepEqual(servers.list.map((r) => r.id), [cherno.id]);
  box.set("");
  assert.equal(servers.filters.search, "", "cleared at once, not after the debounce");
  assert.equal(servers.list.length, 3);
  assert.deepEqual([mapLabel("enoch"), mapLabel("ChernarusPlus"), mapLabel("sakhal"), mapLabel("pripyat"), mapLabel("Esseker")], ["Livonia", "Chernarus", "Sakhal", "Pripyat", "Esseker"]);

  // The settings file comes in late (D-112): its saved filters still win.
  box.set("bravo");
  await advance(t, 200);
  file.resolve(settingsWith({ filters: { map: "deerisle", mods: "modded" } }));
  await settle();
  assert.equal(servers.filters.map, "deerisle");
  assert.equal(servers.filters.mods, "modded");
  assert.equal(servers.filters.search, "bravo", "and the search typed meanwhile stays");
  await advance(t, 1_000);
  assert.equal(app.mock.argsOf("ui_prefs_set").filter((a) => "filters" in a.patch).length, 0, "the search saved nothing");
  box.dispose();
});
