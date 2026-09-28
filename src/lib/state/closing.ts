// Work that has to reach the host before the window goes (row 15, F11): preference
// writes still waiting in their 150 ms batch, and a Settings save inside its 300 ms
// debounce or waiting to be retried. The close button waited for none of them, and
// `beforeunload` has not been seen to run when Tauri destroys the window. The button asks
// here first, for a bounded time. No close handler is registered with Tauri itself:
// with one, the window closes only when the page says so, and a page that had stopped
// answering would leave a window nothing could close.

const pending = new Set<() => Promise<void>>();

/** Registers `f` to run before the window closes; returns the unregister. */
export function beforeClose(f: () => Promise<void>): () => void {
  pending.add(f);
  return () => pending.delete(f);
}

/** Runs everything registered, for at most `limitMs`; never throws. */
export async function flushBeforeClose(limitMs = 1_500): Promise<void> {
  const all = Promise.allSettled([...pending].map((f) => f()));
  await Promise.race([all, new Promise((r) => setTimeout(r, limitMs))]);
}
