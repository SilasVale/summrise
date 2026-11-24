import { useEffect, useState } from "react";
import { api, type Installer } from "./api";

/**
 * The installer the console was given, read once when the surface that asks is drawn.
 *
 * `null` is "nothing to show": the read has not landed, or it failed. That read is the installer's,
 * not `/fleet`'s: a slow or failed machine list must not take away the file that installs a machine.
 * A failed installer read hides the download rather than drawing a door that cannot open, and the
 * page does not poll it — the next visit to the surface is the next read.
 *
 * TWO SURFACES ASK THE SAME QUESTION. Home's installer strip offers the file, and the Machine tab's
 * update card compares the running build with the release that file carries. They read `/installer`
 * through this one hook so the two surfaces cannot disagree about what the release is.
 */
/**
 * **THREE ANSWERS, NOT TWO (the fortieth list).** This hook used to return `Installer | null`, and its
 * `catch` set `null` — the same value "not read yet" had. So a console that FAILED to ask and an
 * account with NO installer were one state, and the page that draws nothing for `null` drew nothing
 * for both. Measured on the live console: with `/fleet` empty and `/installer` answering 500, Home
 * lost the strip entirely (`page-head` + `empty-state`, 848 → 748 px) with no sentence and no retry,
 * on a page whose own comment says the installer "is the next step whether the machine list arrived,
 * is still coming, or failed".
 *
 * The three states are named: `null` is "still reading", `{ state: "read", installer }` is the answer,
 * and `{ state: "failed" }` is a read that did not land. `retry` asks again — the same door the panel's
 * Memory, Settings and Plugins pages offer when their own reads fail.
 */
export type InstallerRead =
  | { state: "reading" }
  | { state: "read"; installer: Installer }
  | { state: "failed" };

export function useInstaller(): InstallerRead & { retry: () => void } {
  const [read, setRead] = useState<InstallerRead>({ state: "reading" });
  // Bumped to ask again. The read is still one-shot per visit unless the operator presses Retry —
  // the installer is an account fact, not a live number, and polling it would be motion without a
  // state change.
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    setRead({ state: "reading" });
    api
      .installer()
      .then((answer) => {
        if (live) setRead({ state: "read", installer: answer });
      })
      .catch(() => {
        if (live) setRead({ state: "failed" });
      });
    return () => {
      live = false;
    };
  }, [attempt]);
  return { ...read, retry: () => setAttempt((count) => count + 1) };
}
