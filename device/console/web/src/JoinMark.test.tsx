import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { JoinLine, JoinMark, joinRecordSentence } from "./JoinMark";
import { JOIN_ONLY_TITLE, JOIN_ONLY_WORD, JOINED_TITLE } from "./joinMarks";

describe("JoinMark", () => {
  it("marks a call only this console still holds", () => {
    render(<JoinMark kind="console" />);
    const chip = screen.getByText(JOIN_ONLY_WORD);
    expect(chip).toHaveClass("chip", "chip-warn");
    // The title is where the sentence lives: the word is two words, the reason is a sentence.
    expect(chip).toHaveAttribute("title", JOIN_ONLY_TITLE);
  });

  it("marks nothing on a call both records hold", () => {
    // Agreement is the ordinary case; a chip on it would be noise on most of the row's own page.
    const { container } = render(<JoinMark kind="one" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("marks nothing on a call the console could not check", () => {
    // The rule this feature must not break: never draw an unchecked row as a lost one.
    const { container } = render(<JoinMark kind="unknown" />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("JoinLine", () => {
  it("says nothing when every call joined", () => {
    const { container } = render(<JoinLine counts={{ one: 4, console: 0, unknown: 0, asked: 4 }} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("says nothing when the window asked about nothing", () => {
    const { container } = render(<JoinLine counts={{ one: 0, console: 0, unknown: 0, asked: 0 }} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("states the rotated rows in one sentence", () => {
    render(<JoinLine counts={{ one: 9, console: 3, unknown: 0, asked: 12 }} />);
    expect(screen.getByText(/this console's record only/)).toHaveClass("hint", "audit-join");
  });
});

describe("joinRecordSentence", () => {
  it("names the machine's copy for a joined call", () => {
    expect(joinRecordSentence("one")).toBe(JOINED_TITLE);
  });

  it("names what is missing for a console-only call", () => {
    expect(joinRecordSentence("console")).toBe(JOIN_ONLY_TITLE);
  });

  it("adds no line for a call that was not checked", () => {
    expect(joinRecordSentence("unknown")).toBeNull();
  });
});
