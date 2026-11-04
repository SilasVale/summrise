import type { Device } from "./api";
import { PluginList } from "./PluginList";
import { RunForm } from "./RunForm";
import { ToolCatalogue } from "./ToolCatalogue";
import { useLiveTools } from "./useLiveTools";
import type { Grant } from "./terminal";

/**
 * What this machine can do, in one place: the tools it advertises, the plugins that add to them, and
 * the form that runs a command through them. The three sections were separate pages (Capabilities,
 * Plugins, and the exec form that led Runs); a plugin's tools were already in the catalogue, and the
 * run form is the one way to make a run, so each is a section of this answer, not a destination.
 *
 * The Device shell owns the one `h1` ("Tools") and the one offline notice above this tab. Each
 * section owns its `h2`; the catalogue's capability groups are the `h3`s under it.
 *
 * THE FRAGMENT IS THE TAB, `#device/tools`. There is no `#device/tools/run` anchor: the surface
 * parser's only sub-path under `tools/` is the open tool's name (`Surface.tool`), so `run` would be
 * read as a tool the machine is missing rather than as this section. The form's stable link is the
 * tab's own.
 */
export function ToolsTab({
  device,
  deviceOnline,
  grant,
  tool,
  onTool,
  onMachine,
  onBrowser
}: {
  device: Device;
  /// The fleet's reachability fact; only `false` is a fact, `undefined` is "no opinion".
  deviceOnline?: boolean;
  /// The same device grant the history page takes; the run form's calls go through it.
  grant: Grant;
  /// The open tool's name, from the fragment: `#device/tools/<tool>`.
  tool: string | null;
  onTool: (name: string | null) => void;
  onMachine: () => void;
  onBrowser: () => void;
}) {
  // One read for both sections: the plugin list needs the tool list to know whether this build can
  // host plugins at all, and asking twice would split one machine fact across two states.
  const live = useLiveTools(device.id);
  const offline = deviceOnline === false || live.offline;

  return (
    <div className="tab-body tools-tab">
      <section className="tools-section" aria-labelledby="tools-on-this-machine">
        <h2 id="tools-on-this-machine">Tools on this machine</h2>
        <ToolCatalogue
          live={live}
          offline={offline}
          tool={tool}
          onTool={onTool}
          onMachine={onMachine}
          onBrowser={onBrowser}
        />
      </section>

      <section className="tools-section" aria-labelledby="tools-plugins">
        <h2 id="tools-plugins">Plugins</h2>
        <PluginList device={device} live={live} offline={offline} onMachine={onMachine} />
      </section>

      <section className="tools-section" aria-labelledby="tools-run">
        <h2 id="tools-run">Run a command on this machine</h2>
        <RunForm grant={grant} deviceOnline={deviceOnline} />
      </section>
    </div>
  );
}
