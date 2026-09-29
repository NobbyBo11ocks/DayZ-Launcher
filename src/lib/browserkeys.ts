// The browser around the app (row 27, approved): WebView2 gives the page a web browser's
// keys and right-click menu, and the launcher is not a web page. F5 and Ctrl+R reloaded the
// whole app, back to News with an open join window gone (row 14, F9); Ctrl+F, Ctrl+P and F3
// opened a find bar and a print dialog over it; right-click offered Back, Refresh, Save as
// and Print. A text field keeps its menu: Cut, Copy and Paste.

/** Whether a key press is one of the browser's own: reload, find, find again, print. Not
 *  with Alt held: Ctrl+Alt is AltGr, which types characters on many keyboards. */
export function isBrowserKey(e: { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean }): boolean {
  if (e.key === "F5" || e.key === "F3") return true;
  if ((!e.ctrlKey && !e.metaKey) || e.altKey) return false;
  const k = e.key.toLowerCase();
  return k === "r" || k === "f" || k === "p";
}

/** The kinds of `<input>` that take typed text. */
const TEXT_TYPES = new Set(["text", "search", "url", "email", "password", "tel", "number"]);

/** Whether a right-click on `target` keeps the WebView's menu: a text field's. Everywhere
 *  else it does nothing. */
export function keepsContextMenu(target: EventTarget | null): boolean {
  const el = target as { tagName?: string; type?: string; isContentEditable?: boolean } | null;
  if (!el) return false;
  if (el.isContentEditable || el.tagName === "TEXTAREA") return true;
  return el.tagName === "INPUT" && TEXT_TYPES.has((el.type || "text").toLowerCase());
}
