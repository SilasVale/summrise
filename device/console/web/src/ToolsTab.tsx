import type { Device } from "./api";
import "./styles/tools-machine.css";
import { PluginList } from "./PluginList";
import { PageHeading } from "./PageHeading";
import { RunForm } from "./RunForm";
import { ToolCatalogue } from "./ToolCatalogue";
import { useLiveTools } from "./useLiveTools";
import type { Grant } from "./terminal";

/** The command action plane, capability catalogue and plugin management share one machine context.
 * The command form remains first in DOM and keyboard order; the catalogue's route still owns its inspection sheet.
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
      <PageHeading level={2} kicker="Capabilities & execution" title="Tools" description="Inspect this machine's capabilities, run a command, and manage installed plugins." />
      <div className="tools-workbench">
        <aside className="tools-action-area" aria-label="Command actions">
          <section className="card tools-section tools-command" aria-labelledby="tools-run">
            <header className="tools-section-header section-heading">
              <h3 id="tools-run">Run a command on this machine</h3>
              <p className="hint">Start one command, then read its exit status and output here.</p>
            </header>
            <RunForm grant={grant} deviceOnline={deviceOnline} osName={device.host?.os?.name} />
          </section>
        </aside>
        <section className="tools-section tools-catalogue" aria-labelledby="tools-on-this-machine">
          <header className="tools-section-header section-heading">
            <h3 id="tools-on-this-machine">Tools on this machine</h3>
            <p className="hint">Select a tool to inspect its inputs, source and effects.</p>
          </header>
          <ToolCatalogue live={live} offline={offline} tool={tool} onTool={onTool} onMachine={onMachine} onBrowser={onBrowser} />
        </section>
        <section className="tools-section tools-plugins" aria-labelledby="tools-plugins">
          <header className="tools-section-header section-heading">
            <h3 id="tools-plugins">Plugins</h3>
            <p className="hint">Installed capabilities, their source directories, and deliberate lifecycle actions.</p>
          </header>
          <PluginList device={device} live={live} offline={offline} onMachine={onMachine} />
        </section>
      </div>
    </div>
  );
}
