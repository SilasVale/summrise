import { useMemo, useState } from "react";
import { filterTools, groupByCapability } from "./capabilities";
import { ToolSheet } from "./ToolSheet";
import { toolsReadState } from "./toolsRead";
import type { LiveToolsState } from "./useLiveTools";

/**
 * Every tool the selected machine advertises, grouped by capability, with the live `tools/list` as
 * the source. The section never shows a tool the device would refuse as unknown: a failed read is a
 * banner with Retry, not an empty catalogue, and an older build's list is what it is.
 *
 * This is the catalogue half of the Tools tab; the tab's `<h2>` and the plugin section live in
 * `ToolsTab.tsx`. The offline notice is the Device shell's, above both sections, so this one does
 * not repeat it.
 *
 * THE HEADER NEVER CLAIMS A READ IS IN PROGRESS UNLESS ONE IS. `Reading the tool list…` is the
 * reading state alone; a finished read that brought no list says there is no list, and an offline
 * machine's header says so too, because a read that cannot finish must not read as pending.
 */
export function ToolCatalogue({
  live,
  offline,
  tool,
  onTool,
  onMachine,
  onBrowser
}: {
  live: LiveToolsState;
  /// The machine's state from the shell (`deviceOnline === false` or the failed read's own answer).
  offline: boolean;
  /// The open tool's name, from the fragment; null closes the sheet.
  tool: string | null;
  onTool: (name: string | null) => void;
  onMachine: () => void;
  onBrowser: () => void;
}) {
  const [query, setQuery] = useState("");
  const groups = useMemo(
    () => groupByCapability(filterTools(live.tools, query)),
    [live.tools, query]
  );
  const capabilities = useMemo(() => groupByCapability(live.tools).length, [live.tools]);
  const openTool = tool ? live.tools.find((entry) => entry.name === tool) ?? null : null;
  // One reading/failed/ready state for the header and the body, so an unfinished read cannot be
  // drawn as a finished empty catalogue.
  const read = toolsReadState(live);
  const missingTool = tool !== null && read !== "reading" && live.tools.length > 0 && !openTool;
  // **THE NOTICE BELOW IS THE FAILURE'S ONE STATEMENT (audit-43 F9).** The subtitle used to say
  // `No tool list from this machine.` above a notice that said the same thing, and an offline
  // machine got `…it is offline.` above a body sentence that repeated it. When there is no list,
  // the read's state is the notice's or the body's subject; the subtitle is silent.
  const subtitle =
    live.tools.length > 0
      ? `${live.tools.length} tools · ${capabilities} capabilit${capabilities === 1 ? "y" : "ies"}`
      : read === "reading"
        ? "Reading the tool list…"
        : read === "failed" || offline
          ? null
          : "No tool list from this machine.";

  return (
    <>
      <div className="tools-section-head">
        {subtitle !== null && <p className="page-sub">{subtitle}</p>}
        <label className="search-field">
          <span className="sr-only">Search tools</span>
          <input
            type="search"
            aria-label="Search tools"
            placeholder="Search tools"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
      </div>

      {read === "reading" ? (
        <div className="skeleton-list" aria-label="Reading the tool list" role="status">
          {[0, 1, 2].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      ) : offline && live.tools.length === 0 ? (
        // **NO RETRY AN OFFLINE MACHINE CANNOT USE (audit-43 F9).** The shell above already says
        // the machine is away and dates it; a press here could not read anything until it returns,
        // and the next read after it does is the page's own mount or the operator's next visit.
        <section className="empty-state">
          <p className="hint">The console has no tool list from this machine yet.</p>
        </section>
      ) : read === "failed" ? (
        <div className="notice" role="status" title={live.error ?? undefined}>
          {/* The envelope's raw sentence is evidence, not the page's sentence: it stays the
              notice's title, where the one Retry sits beside a consequence the operator can read. */}
          <span>The console could not read this machine's tools.</span>
          <button type="button" onClick={() => live.retry()}>
            Retry
          </button>
        </div>
      ) : live.tools.length === 0 ? (
        <section className="empty-state">
          <h3>No tools</h3>
          <p className="hint">
            This machine is running a build that advertises no tools. Open Machine to see what it is
            running.
          </p>
          <button type="button" onClick={onMachine}>
            Open Machine
          </button>
        </section>
      ) : (
        <>
          {live.stale && (
            <div className="notice" role="status">
              <span>
                Showing the last tool list the console read; it could not be refreshed.
              </span>
              <button type="button" onClick={() => live.retry()}>
                Retry
              </button>
            </div>
          )}
          {missingTool && (
            <div className="notice" role="status">
              <span>That tool is not on this machine.</span>
              <button type="button" onClick={() => onTool(null)}>
                Dismiss
              </button>
            </div>
          )}
          {groups.length === 0 ? (
            <section className="empty-state">
              <p>No tool matches "{query}".</p>
              <button type="button" onClick={() => setQuery("")}>
                Clear the search
              </button>
            </section>
          ) : (
            <div className="cap-groups">
              {groups.map((group) => (
                <section className="cap-group" key={group.name} aria-labelledby={`cap-${group.name}`}>
                  <div className="cap-group-head">
                    <h3 id={`cap-${group.name}`}>{group.name}</h3>
                    <span className="chip">
                      {group.plugin ? `Plugin · ${group.plugin}` : "Built-in"}
                    </span>
                    <span className="cap-summary">
                      {group.tools.length} tool{group.tools.length === 1 ? "" : "s"}
                      {group.destructive > 0
                        ? ` · ${group.destructive} can change the machine`
                        : ""}
                    </span>
                    {group.name === "browser" && (
                      <button type="button" className="mini cap-open-browser" onClick={onBrowser}>
                        Open browser
                      </button>
                    )}
                  </div>
                  <p className="cap-job">{group.description}</p>
                  <div className="tool-list">
                    {group.tools.map((entry) => (
                      <button
                        type="button"
                        className="tool-row"
                        key={entry.name}
                        onClick={() => onTool(entry.name)}
                      >
                        <span className="tool-name mono">{entry.name}</span>
                        {entry.description && (
                          <span className="tool-description">{entry.description}</span>
                        )}
                        <span className="tool-chevron" aria-hidden="true">
                          ›
                        </span>
                      </button>
                    ))}
                  </div>
                </section>
              ))}
            </div>
          )}
        </>
      )}

      {openTool && (
        <ToolSheet key={openTool.name} tool={openTool} onClose={() => onTool(null)} />
      )}
    </>
  );
}
