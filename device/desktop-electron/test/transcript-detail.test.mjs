import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../panel/activity.html", import.meta.url), "utf8");

test("the transcript's expand fetches the run and names the session it came from", () => {
  assert.match(html, /\/api\/runs\//, "the expand asks the shim for the run record");
  assert.match(html, /data-run="\$\{escapeHtml\(c\.runId\)\}"/, "a row carries its run id");
  assert.match(html, /command<\/span><span class="v">/, "the expand prints the full command");
  assert.match(html, /stdout \$\{run\.stdout_bytes/, "the expand reports the output sizes it has");
  assert.match(html, /run\.stdout_preview/, "the expand renders the preview the history kept");
  assert.match(html, /credential shapes replaced by \*\*\*/, "the preview says what it scrubbed");
  assert.match(html, /open terminal/, "a terminal row links the terminal");
});

test("a tool filter narrows the feed without losing the counts", () => {
  assert.match(html, /id="filter"/, "the header has a filter");
  assert.match(html, /groupOf\(c\.tool\) === filter/, "the filter reads the tool's group");
  assert.match(html, /<option value="runs">runs<\/option>/, "runs is one of the groups");
  assert.match(html, /calls\.length|order\.map/, "the counts stay over every call, not the filter");
});
