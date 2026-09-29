// Escape's order (row 27). The grid, a page closing its details pane, the Direct connect
// popover and the video player act on the key and mark it handled; the notices take it
// only when nothing did. One Escape used to close the details pane and clear every notice
// with it, a notice like "Saved data was damaged and moved to …" gone unread.

/** A modal that owns its Escape: the join window and its fallback, the first-run and
 *  What's new windows. */
export const MODAL = '[role="dialog"][aria-modal="true"], [role="alertdialog"][aria-modal="true"]';

/** Whether the notices take this Escape, asked once every other handler has had it. Not
 *  one something acted on, not behind a modal, and not in a field, which keeps its own. */
export function noticesTakeEscape(e: { key: string; defaultPrevented: boolean; target: EventTarget | null }, modalOpen: boolean): boolean {
  if (e.key !== "Escape" || e.defaultPrevented || modalOpen) return false;
  const tag = (e.target as { tagName?: string } | null)?.tagName;
  return tag !== "INPUT" && tag !== "TEXTAREA" && tag !== "SELECT";
}
