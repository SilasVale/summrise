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
export function useInstaller(): Installer | null {
  const [installer, setInstaller] = useState<Installer | null>(null);
  useEffect(() => {
    let live = true;
    api
      .installer()
      .then((answer) => {
        if (live) setInstaller(answer);
      })
      .catch(() => {
        if (live) setInstaller(null);
      });
    return () => {
      live = false;
    };
  }, []);
  return installer;
}
