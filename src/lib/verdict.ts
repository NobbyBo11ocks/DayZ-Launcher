// A server's check in the player's words, for the details pane. The host's reason strings
// are the rules' own ("INFO 40 vs PLAYER 3", "all_young=false, named=true", written by
// `judge` and `continuity` in browser/verify.rs) and were shown as they were — the same
// leak as D-234, on every server (D-248). Each sentence keys on that wording, and
// tests/front/verdict.test.mjs holds one reason of every kind: a rule reworded on one
// side only fails there instead of falling through to the general line.
import type { Verdict, Verification } from "./types";

/** The verdict explained: what the check found, in a sentence. */
export function explainVerdict(v: Verification): string {
  const claim = v.reported >= 0 ? String(v.reported) : null;
  const n = v.verified ?? 0;
  switch (v.verdict) {
    case "verified":
      if (v.reason.startsWith("INFO reports 0")) return "The server reports nobody on it and did not share its player list.";
      if (v.reason.includes("INFO did not answer")) return `${n} counted; the server's own number did not arrive.`;
      return claim != null ? `${n} counted; the server claims ${claim}.` : `${n} counted.`;
    case "inflated":
      if (v.reason.includes("zero-length")) return `${n} real player${n === 1 ? "" : "s"}; the rest of its list are fake entries with no play time.`;
      return `The server claims ${claim ?? "more"} players; ${n} ${n === 1 ? "is" : "are"} actually connected.`;
    case "unverifiable":
      return `The server claims ${claim ?? "some"} players but does not share its player list.`;
    case "offline":
      return "The server did not answer at all.";
    case "synthetic": {
      if (v.reason.includes("named=true")) return "The player list looks fake: its entries carry names, which real DayZ lists never do.";
      // R13, R14 and R11's invariant (D-314), in the words approved for them (D-315).
      if (v.reason.includes("entries on a server of")) return "The player list looks fake: it lists more players than the server has slots.";
      if (v.reason.includes("under a second beside")) return "The player list looks fake: several players joined within the same second as long-running ones.";
      if (v.reason.includes("missing from the list")) return "The player list looks fake: players it lists as older were not there at the previous check.";
      if (v.reason.includes("carried over between checks")) return "The player list looks fake: the sessions seen at the previous check did not carry over.";
      // A standing verdict, from whichever rule gave it (row 28).
      if (v.reason.includes("earlier check")) return "The player list looked fake at an earlier check; waiting for a check close enough to compare.";
      const m = /^(\d+) entries, (\d+) distinct/.exec(v.reason);
      if (m) return `The player list looks fake: ${m[1]} entries with only ${m[2]} different session length${m[2] === "1" ? "" : "s"}.`;
      return "The player list looks fake.";
    }
    default:
      return v.reason;
  }
}

const LABEL: Record<Verdict, string> = {
  verified: "Verified player count",
  inflated: "Inflated player count",
  unverifiable: "Player list not shared",
  synthetic: "Fake player list",
  offline: "Not answering",
};

/** The verdict's heading. "Verified player count" over a check that counted nobody, because
 *  the player list never came and the server said 0, promised a count there was not (D-268). */
export const verdictHeading = (v: Verification): string =>
  v.verdict === "verified" && v.reason.startsWith("INFO reports 0") ? "Empty server" : LABEL[v.verdict];
