// What the Logs page's Copy puts on the clipboard: the text a player pastes into a bug
// report. The page shows the newest first at local times without a date; a report reads
// oldest first, at the file's own UTC times, under a line with what a fix needs to know.
// It was the page's list as it stood, with none of the version, Windows or WebView2
// (row 25).

export type LogLine = { at: number; level: "info" | "warn" | "error"; target: string; message: string };
/** `app_info`'s answer, as far as a report reads it. */
export type ReportFacts = { version: string; windows?: string | null; webview?: string | null; elevated?: boolean };

/** `2026-09-22 12:34:56.789` in UTC, the file's own stamp (log.rs `stamp`). */
export function stamp(ms: number): string {
  return new Date(ms).toISOString().replace("T", " ").replace("Z", "");
}

/** The header line and the entries, oldest first. */
export function formatReport(lines: readonly LogLine[], facts: ReportFacts | null): string {
  const head = [
    `DZSA CrayZ Launcher ${facts?.version ?? "?"}`,
    `Windows ${facts?.windows ?? "?"}`,
    `WebView2 ${facts?.webview ?? "?"}`,
    facts?.elevated ? "as administrator" : "not as administrator",
    "times UTC",
  ].join(" · ");
  const body = [...lines].sort((a, b) => a.at - b.at).map((l) => `${stamp(l.at)} ${l.level.toUpperCase().padEnd(5)} ${l.target} ${l.message}`);
  return [head, ...body].join("\n");
}
