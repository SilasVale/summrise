// The getting-started content, in ONE place, as data.
//
// WHY IT IS DATA AND NOT MARKUP. The operator's complaint was "I do not know how to use the
// features", and the reason a guide like this rots is that it is prose written next to code that
// then changes: it goes on describing a page that was merged away or a command that was pruned
// (this loop has pruned plenty). Keeping it as a list lets a test assert that every page named here
// still EXISTS (`PAGE_ICONS`), and that each step still has the parts a reader needs.
//
// WHAT IT EXPLAINS, and nothing else: the three things this product is for — a real console into
// the machine, watches that say when something stops answering, and an AI working on it with a
// human in the loop. A guide that lists every button is a manual; this is the first five minutes.

import type { Page } from "../components/Shell";

/** Bump to show the guide again after it changes: the stored value is compared to this. */
export const GETTING_STARTED_VERSION = "1";

/** The localStorage key. Versioned by VALUE, not by key, so one key is all that is ever written. */
export const GETTING_STARTED_KEY = "valeGettingStarted";

export interface Step {
  /** Which page the step sends you to — must exist in PAGE_ICONS. */
  page: Page;
  /** The chip the reader looks for in the rail. Short: it is a label, not a sentence. */
  where: string;
  title: string;
  detail: string;
  /** The exact thing to TYPE, rendered monospace. ONLY for a real command — a click path set in
   *  monospace reads as something to type, which is how step 1 and step 3 both shipped wrong in the
   *  first screenshot review of this card. */
  action?: string;
  /** A click path, written as prose because it is not something you can type. */
  click?: string;
}

export const STEPS: Step[] = [
  {
    page: "terminal",
    where: "Terminal",
    title: "Open a console",
    detail:
      "The + button starts a session: PTY is this Windows machine, SSH is a box on the network, Serial is a COM port. Type into it and it is recorded.",
    // NOT `action`: that field renders in the mono "type this" style, and this is a CLICK path —
    // a command chip the reader cannot type is worse than no chip (caught in the first screenshot
    // review of this card).
    click: "Start with + in the session bar",
  },
  {
    page: "settings",
    where: "Settings · Reachability",
    title: "Watch what must keep answering",
    detail:
      "Add a host:port — or a full URL plus the text the page must contain — and the chip in the status bar turns red the moment it stops answering, from any page.",
    action: "vale monitor add 192.168.1.1:22",
  },
  {
    page: "activity",
    where: "Activity",
    title: "Let an AI work on it, with you in the loop",
    detail:
      "Connect your AI client to this device's MCP endpoint. Every run it makes shows up here, each session keeps its own evidence, and anything that needs a human waits for your approval.",
    click: "Open Activity → pick the run",
  },
];

/** The one-sentence answer to "what is this?", above the steps. */
export const GETTING_STARTED_LEAD =
  "One machine, seen from anywhere: a real console into it, watches that tell you when something stops answering, and an AI that can work on it while you watch.";

/** Where the guide can be reopened from — shown in the footer, so it is never a dead end. */
export const GETTING_STARTED_REOPEN = "Reopen this any time from the ? button at the foot of the rail.";

/** Should the guide open for this stored value? Pure, so the rule is testable without a browser. */
export function shouldShowGuide(stored: string | null): boolean {
  return stored !== GETTING_STARTED_VERSION;
}
