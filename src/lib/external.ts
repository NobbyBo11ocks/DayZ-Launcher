// External links open in the player's browser (D-099); the WebView itself never navigates
// away. Through the host's `open_link`, which opens a page as the signed-in player while
// the launcher runs as administrator, and logs why one could not be opened (row 27).
import { invoke } from "@tauri-apps/api/core";

/** Opens `url` in the player's browser; `false` when it could not, the reason in the log. */
export async function openExternal(url: string): Promise<boolean> {
  try {
    await invoke("open_link", { url });
    return true;
  } catch {
    return false;
  }
}

/** For `<a href … onclick={external}>`. */
export function external(e: MouseEvent) {
  const a = e.currentTarget as HTMLAnchorElement | null;
  if (!a?.href) return;
  e.preventDefault();
  void openExternal(a.href);
}
