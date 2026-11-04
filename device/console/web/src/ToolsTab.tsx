import type { Device } from "./api";
import { PluginList } from "./PluginList";
import { ToolCatalogue } from "./ToolCatalogue";
import { useLiveTools } from "./useLiveTools";

/**
 * What this machine can do, in one place: the tools it advertises, then the plugins that add to
 * them. The two sections were rail destinations of their own; a plugin's tools were already in the
 * catalogue, so the management list is the catalogue's source, not a second answer.
 *
 * The Device shell owns the one `h1` ("Tools") and the one offline notice above this tab. Each
 * section owns its `h2`; the catalogue's capability groups are the `h3`s under it.
 */
export function ToolsTab({
  device,
  deviceOnline,
  tool,
  onTool,
  onMachine,
  onBrowser
}: {
  device: Device;
  /// The fleet's reachability fact; only `false` is a fact, `undefined` is "no opinion".
  deviceOnline?: boolean;
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
    </div>
  );
}
