// What a page had when it was left, for the rest of the session (row 27, approved): Mods'
// search, sort and view, Friends' Show offline and Logs' Problems only started over each
// time the page was opened again. Not written to the settings file: a new session starts
// as before. Plain values; each page copies its own in when it opens and back as they
// change.

export type ModsSortKey = "name" | "size" | "updated" | "servers";
export type ModsView = "all" | "updates" | "unused" | "nojunction";

export const pageState = {
  mods: { search: "", sort: { key: "size" as ModsSortKey, dir: -1 as 1 | -1 }, view: "all" as ModsView },
  friends: { showOffline: false },
  logs: { onlyProblems: false },
};
