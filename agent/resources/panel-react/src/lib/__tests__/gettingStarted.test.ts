// The guide is CONTENT, and content rots: it goes on naming a page that was merged away (this
// repo has merged and deleted plenty) or a command that was pruned. These tests are the pin — the
// guide may only talk about things that exist, and it must say enough for a reader to act.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { PAGE_ICONS } from "../../components/IconRail";
import {
  GETTING_STARTED_KEY,
  GETTING_STARTED_LEAD,
  GETTING_STARTED_REOPEN,
  GETTING_STARTED_VERSION,
  STEPS,
  shouldShowGuide,
} from "../gettingStarted";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..", "..");

describe("getting started: the content is checked, not trusted", () => {
  it("sends the reader only to pages that exist", () => {
    for (const step of STEPS) {
      expect(
        Object.keys(PAGE_ICONS),
        `step "${step.title}" points at page "${step.page}", which is not in the rail`,
      ).toContain(step.page);
      // The chip is what the reader looks for in the rail, so it may not be empty.
      expect(step.where.trim().length).toBeGreaterThan(2);
    }
  });

  it("every step is actionable: a title, a reason, and a way in", () => {
    expect(STEPS.length).toBeGreaterThanOrEqual(3);
    for (const step of STEPS) {
      expect(step.title.length).toBeGreaterThan(4);
      // Three lines of rationale is the budget; a wall of text is what a guide must not be.
      expect(step.detail.length).toBeGreaterThan(40);
      expect(step.detail.length).toBeLessThan(260);
      expect(Boolean(step.action) !== Boolean(step.click), `"${step.title}" needs exactly one of action/click`).toBe(true);
    }
  });

  it("a monospace ACTION is a command the CLI actually has", () => {
    // The one real command in the guide. If the CLI ever loses `monitor add`, the guide must not go
    // on telling people to type it — so this reads the shipped CLI and checks the usage line.
    const cli = readFileSync(path.join(ROOT, "vale-agent-npm", "bin", "vale.js"), "utf8");
    const commands = STEPS.flatMap((s) => (s.action ? [s.action] : []));
    expect(commands.length, "the guide should show at least one real command").toBeGreaterThan(0);
    for (const cmd of commands) {
      const m = /^vale\s+([a-z]+)\s+([a-z]+)/.exec(cmd);
      expect(m, `"${cmd}" is not shaped like a vale command`).not.toBeNull();
      const [, verb, sub] = m!;
      // THE USAGE LINE IS THE CONTRACT: it is what the CLI prints to somebody who asks, so it is
      // what the guide must agree with. (Asserting on the dispatcher's internal shape was my first
      // attempt and it was wrong about the code rather than about the product.)
      // GREEDY to the last bracket on the line: the usage lists NESTED options
      // (`[list [--json] | add …]`), and a non-greedy match stops at `[--json`.
      const usage = cli.match(new RegExp(`usage: vale ${verb} \\[(.*)\\]`));
      expect(usage, `the CLI prints no usage line for \`vale ${verb}\``).not.toBeNull();
      expect(usage![1], `\`vale ${verb}\` no longer offers \`${sub}\``).toContain(sub);
    }
  });

  it("opens once per content version, and only then", () => {
    expect(shouldShowGuide(null)).toBe(true);
    expect(shouldShowGuide("")).toBe(true);
    expect(shouldShowGuide(GETTING_STARTED_VERSION)).toBe(false);
    // An older version re-opens it: the card changed, so the reader gets to see the change.
    expect(shouldShowGuide("0")).toBe(true);
    expect(GETTING_STARTED_KEY).toBe("valeGettingStarted");
    // The footer must tell the reader how to get back — a dismissible card with no way back is a
    // dead end.
    expect(GETTING_STARTED_REOPEN).toContain("?");
    expect(GETTING_STARTED_LEAD.length).toBeGreaterThan(60);
  });
});
