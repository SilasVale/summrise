import { useMemo, useState } from "react";
import type { Device } from "./api";
import { filterTools, groupByCapability } from "./capabilities";
import { DevicePicker } from "./DevicePicker";
import { ToolSheet } from "./ToolSheet";
import { useLiveTools } from "./useLiveTools";

/**
 * Every tool the selected machine advertises, grouped by capability, with the live `tools/list` as
 * the source. The page never shows a tool the device would refuse as unknown: a failed read is a
 * banner with Retry, not an empty catalogue, and an older build's list is what it is.
 */
export function CapabilitiesPage({
  devices,
  device,
  tool,
  onChoose,
  onTool,
  onMachine,
  onActivity,
  onBrowser
}: {
  devices: Device[];
  device: Device | null;
  /// The open tool's name, from the fragment; null closes the sheet.
  tool: string | null;
  onChoose: (id: string) => void;
  onTool: (name: string | null) => void;
  onMachine: () => void;
  onActivity: () => void;
  onBrowser: () => void;
}) {
  const live = useLiveTools(device?.id ?? null);
  const [query, setQuery] = useState("");
  const groups = useMemo(
    () => groupByCapability(filterTools(live.tools, query)),
    [live.tools, query]
  );
  const capabilities = useMemo(() => groupByCapability(live.tools).length, [live.tools]);
  const openTool = tool ? live.tools.find((entry) => entry.name === tool) ?? null : null;
  const missingTool = tool !== null && !live.loading && live.tools.length > 0 && !openTool;
  const grant = device ? { deviceId: device.id, envelope: null } : null;
  const name = device?.name ?? "";

  return (
    <div className="page capabilities-page">
      <header className="page-head">
        <div>
          <h1>Capabilities</h1>
          <p className="page-sub">
            {live.tools.length > 0
              ? `${live.tools.length} tools · ${capabilities} capabilit${capabilities === 1 ? "y" : "ies"}`
              : device
                ? "Reading the tool list…"
                : "Choose a machine to read its tools."}
          </p>
        </div>
        {device && (
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
        )}
      </header>

      {!device ? (
        <section className="empty-state">
          <h2>Choose a machine</h2>
          <DevicePicker devices={devices} onChoose={onChoose} />
        </section>
      ) : live.loading && live.tools.length === 0 ? (
        <div className="skeleton-list" aria-label="Reading the tool list" role="status">
          {[0, 1, 2].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      ) : live.error && live.tools.length === 0 ? (
        <div className="notice" role="status">
          <span>
            {live.offline
              ? `${name || "This machine"} is not connected. The console keeps trying; open the machine's panel if it stays offline.`
              : `The console could not read this machine's tools: ${live.error}`}
          </span>
          <button type="button" onClick={() => live.retry()}>
            Retry
          </button>
        </div>
      ) : live.tools.length === 0 ? (
        <section className="empty-state">
          <h2>No tools</h2>
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
                    <h2 id={`cap-${group.name}`}>{group.name}</h2>
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

      {openTool && grant && (
        <ToolSheet
          key={openTool.name}
          tool={openTool}
          grant={grant}
          onClose={() => onTool(null)}
          onActivity={onActivity}
        />
      )}
    </div>
  );
}
