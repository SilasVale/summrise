import { describe, expect, it } from "vitest";
import { auditSourceSentence } from "./auditSource";

// The audit source line is the companion to the cap sentence, not a second cap sentence: it says
// which file the window is read from and how far back that file goes. Every figure comes from the
// server; a fact the file could not give is absent, never guessed.

const source = {
  file: "audit.jsonl",
  bytes: 8_808_038,
  oldest_ts_ms: new Date(2026, 10, 2, 9, 12).getTime(),
  newest_ts_ms: new Date(2026, 10, 5, 17, 0).getTime(),
  max_bytes: 8_388_608,
  rotations: 1,
  rotated: {
    file: "audit.1.jsonl",
    oldest_ts_ms: new Date(2026, 10, 2, 3, 10).getTime()
  }
};

describe("auditSourceSentence", () => {
  it("states the file, its size, where its lines start and the older generation", () => {
    expect(auditSourceSentence(source)).toBe(
      "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12; rotated once, older lines start 11-02 03:10."
    );
  });

  it("omits the rotation clause when no predecessor exists", () => {
    expect(auditSourceSentence({ ...source, rotated: undefined })).toBe(
      "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12."
    );
  });

  it("says only that it rotated when the older file's oldest line cannot be read", () => {
    expect(auditSourceSentence({ ...source, rotated: { file: "audit.1.jsonl" } })).toBe(
      "Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12; rotated once."
    );
  });

  it("drops a moment the server could not read instead of inventing one", () => {
    expect(auditSourceSentence({ file: "audit.jsonl", bytes: 0 })).toBe(
      "Reading audit.jsonl — 0 B."
    );
  });

  it("says nothing when there is no file to read", () => {
    expect(auditSourceSentence(null)).toBeNull();
    expect(auditSourceSentence(undefined)).toBeNull();
    expect(auditSourceSentence({})).toBeNull();
  });
});
