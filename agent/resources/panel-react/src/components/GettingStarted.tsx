// GettingStarted — the first five minutes, shown once and reopenable for ever after.
//
// DESIGN DECISIONS, because they are the point of this component and not decoration:
//
//  * IT ANSWERS "WHERE DO I GO", not "what buttons exist". Each step carries the rail chip the
//    reader must look for, and the chip is a BUTTON that takes them there — a guide that only
//    describes a place is a guide the reader has to translate.
//  * ONE COLUMN, NUMBERED, with the number's rail carrying the eye down. A grid of cards would
//    say "here are four unrelated things"; this is a sequence, and it is drawn as one.
//  * THE CARD IS THE PRODUCT'S OWN SURFACE: the panel's tokens, radii and primary button, so it
//    looks like Vale in both themes instead of like a dialog that arrived from elsewhere.
//  * IT IS DISMISSIBLE AND NEVER FORCED AGAIN, and Esc/backdrop/button all dismiss it. A modal
//    that traps somebody who does not want it is worse than no guide; the rail keeps a `?`.
//  * FOCUS LANDS ON THE PRIMARY BUTTON and the card is a labelled `aria-modal` dialog, so a
//    keyboard or screen-reader user is not left behind the overlay.

import { useEffect, useRef } from "react";
import { Icon } from "../ui/Icon";
import type { Page } from "./Shell";
import {
  GETTING_STARTED_LEAD,
  GETTING_STARTED_REOPEN,
  STEPS,
} from "../lib/gettingStarted";

export function GettingStarted({
  onClose,
  onGoTo,
}: {
  onClose: () => void;
  /** Jump to the page a step is about. Without it the chips are plain labels. */
  onGoTo?: (page: Page) => void;
}) {
  const done = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    done.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="gs-backdrop"
      // The backdrop closes it too: the click target is the backdrop itself, so a click INSIDE
      // the card never dismisses anything.
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="gs-card" role="dialog" aria-modal="true" aria-labelledby="gs-title">
        <div className="gs-head">
          <h2 id="gs-title">Getting started</h2>
          <button className="gs-close" onClick={onClose} aria-label="Close the guide">
            <Icon name="close" />
          </button>
        </div>
        <p className="gs-lead">{GETTING_STARTED_LEAD}</p>

        <ol className="gs-steps">
          {STEPS.map((step, i) => (
            <li key={step.title} className="gs-step">
              <span className="gs-num" aria-hidden="true">
                {i + 1}
              </span>
              <div className="gs-body">
                <div className="gs-line">
                  <h3>{step.title}</h3>
                  {onGoTo ? (
                    <button className="gs-where" onClick={() => onGoTo(step.page)}>
                      {step.where}
                      <Icon name="chevron" />
                    </button>
                  ) : (
                    <span className="gs-where static">{step.where}</span>
                  )}
                </div>
                <p>{step.detail}</p>
                {step.action && <code className="gs-action">{step.action}</code>}
                {step.click && <span className="gs-click">{step.click}</span>}
              </div>
            </li>
          ))}
        </ol>

        <div className="gs-foot">
          <span className="gs-reopen">{GETTING_STARTED_REOPEN}</span>
          <button className="btn primary" ref={done} onClick={onClose}>
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}
