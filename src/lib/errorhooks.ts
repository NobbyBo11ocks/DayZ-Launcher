// Imported first by main.ts, so uncaught errors and rejected promises reach the log
// before any store is built. Installed from App.svelte, they came after every store
// module had already run: a throw while one was built left a blank window and no line
// (row 25).
import { installErrorHooks } from "./log";

installErrorHooks();
