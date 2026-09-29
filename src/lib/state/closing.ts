// Work that has to reach the host before the window goes (row 15, F11): preference
// writes still waiting in their 150 ms batch, and a Settings save inside its 300 ms
// debounce or waiting to be retried. `beforeunload` has not been seen to run when Tauri
// destroys the window. Every way of closing it — its own button, Alt+F4, the taskbar's
// Close — reaches the host's close handler, which asks here (`app:closing`) and closes
// the window when this answers, or after 1.5 s whatever the page does, so a page that
// has stopped answering cannot keep it open (row 27). Only the title bar's button used
// to wait for this.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const pending = new Set<() => Promise<void>>();

/** Registers `f` to run before the window closes; returns the unregister. */
export function beforeClose(f: () => Promise<void>): () => void {
  pending.add(f);
  return () => pending.delete(f);
}

/** Runs everything registered, for at most `limitMs`; never throws. */
export async function flushBeforeClose(limitMs = 1_500): Promise<void> {
  // Each behind a promise: one that threw before its first await rejected the whole flush,
  // and the close waited on it (row 27).
  const all = Promise.allSettled([...pending].map((f) => Promise.resolve().then(f)));
  await Promise.race([all, new Promise((r) => setTimeout(r, limitMs))]);
}

/** Answers the host's `app:closing` with the flush, then `close_ready`. Under the host's
 *  own 1.5 s, so the answer normally closes the window. Returns the unlisten. */
export function answerCloseRequests(): () => void {
  const off = listen("app:closing", () => {
    void flushBeforeClose(1_200)
      .then(() => invoke("close_ready"))
      .catch(() => {
        /* the host closes the window on its own after 1.5 s */
      });
  });
  return () => void off.then((f) => f());
}
