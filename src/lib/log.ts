// Frontend half of the diagnostic log (D-158). Everything here ends up in the same
// file as the Rust side, tagged `ui:<area>`, so a session reads as one story.
//
// Deliberately quiet: errors, failed commands, and a handful of one-off timings. Nothing
// on a per-row, per-frame or per-keystroke path.
import { invoke } from "@tauri-apps/api/core";

type Level = "info" | "warn" | "error";

/** Never throws and never awaits the caller: logging must not change behaviour. */
export function log(level: Level, target: string, message: string): void {
  void invoke("log_ui", { level, target, message }).catch(() => {});
}

export const logInfo = (target: string, message: string) => log("info", target, message);
export const logWarn = (target: string, message: string) => log("warn", target, message);
export const logError = (target: string, message: string) => log("error", target, message);

/** Turns anything thrown into one line, including Tauri's error objects. */
export function describe(e: unknown): string {
  if (e instanceof Error) return `${e.name}: ${e.message}`;
  if (typeof e === "string") return e;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

/** Wraps an invoke so a failure is recorded with its command name, then rethrown. */
export async function invokeLogged<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    logError("ipc", `${command} failed: ${describe(e)}`);
    throw e;
  }
}

let installed = false;

/** Catches what the app itself does not: uncaught errors and rejected promises. */
export function installErrorHooks(): void {
  if (installed) return;
  installed = true;

  window.addEventListener("error", (e) => {
    const where = e.filename ? ` at ${e.filename}:${e.lineno}:${e.colno}` : "";
    logError("window", `${e.message}${where}`);
  });

  window.addEventListener("unhandledrejection", (e) => {
    logError("promise", describe(e.reason));
  });

  // Console errors from libraries that swallow their own failures.
  const original = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    original(...args);
    logError("console", args.map(describe).join(" ").slice(0, 500));
  };
}
