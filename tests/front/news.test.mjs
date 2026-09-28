// The News page's feed (src/lib/state/news.svelte.ts): a fetch that failed offline.
import test from "node:test";
import assert from "node:assert/strict";
import { advance, freshApp, settle } from "./harness.mjs";

const OFFLINE = "The news could not be loaded: no connection to Steam. It is tried again when the connection is back.";

test("a failed news fetch is tried again when the connection or the window comes back, at most once a minute (D-302 (11), row 14 F6)", async (t) => {
  const app = await freshApp(t);
  app.mock.handle("news_cached", { items: [] });
  app.mock.fail("news_fetch", OFFLINE, { always: true });
  const { news } = await app.load("state/news.svelte");
  await news.start();
  const fetches = () => app.mock.count("news_fetch");
  assert.equal(fetches(), 1);
  assert.equal(news.error, OFFLINE);

  app.fire("focus"); // the player comes back to the window, still offline
  await settle();
  assert.equal(fetches(), 2);
  await advance(t, 10_000);
  app.fire("online");
  await settle();
  assert.equal(fetches(), 2, "not twice inside a minute");

  await advance(t, 50_000);
  app.mock.handle("news_fetch", { items: [{ gid: "1", title: "Update 1.29.2", url: "https://example.invalid/", author: "Bohemia", feed: "Community Announcements", official: true, date: 1_790_000_000, summary: "", update: true }] });
  app.fire("online");
  await settle();
  assert.equal(fetches(), 3, "the connection is back");
  assert.equal(news.error, null);
  assert.equal(news.items.length, 1);

  await advance(t, 120_000);
  app.fire("focus");
  await settle();
  assert.equal(fetches(), 3, "nothing to retry once it worked");
  news.stop();
});
