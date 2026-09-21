// External links open in the default browser through the opener plugin (D-099);
// the WebView itself never navigates away. Use as `<a href … onclick={external}>`.
import { openUrl } from "@tauri-apps/plugin-opener";

export function external(e: MouseEvent) {
  const a = e.currentTarget as HTMLAnchorElement | null;
  if (!a?.href) return;
  e.preventDefault();
  void openUrl(a.href).catch(() => {});
}
