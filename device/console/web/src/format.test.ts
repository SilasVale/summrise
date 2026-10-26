import { describe, expect, it } from "vitest";
import { message, shortId } from "./format";

describe("shortId", () => {
  it("leaves an id of twelve characters alone", () => {
    expect(shortId("0123456789ab")).toBe("0123456789ab");
  });

  it("cuts a longer id to twelve characters and says it cut", () => {
    expect(shortId("0123456789abc")).toBe("0123456789ab…");
  });

  it("leaves a short id alone", () => {
    expect(shortId("dev")).toBe("dev");
  });
});

describe("message", () => {
  it("is an Error's message", () => {
    expect(message(new Error("x"))).toBe("x");
  });

  it("is a string unchanged", () => {
    expect(message("x")).toBe("x");
  });

  it("is anything else as a string", () => {
    expect(message(42)).toBe("42");
  });
});
