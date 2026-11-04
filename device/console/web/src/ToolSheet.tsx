import { useMemo } from "react";
import type { LiveTool } from "./api";
import { summariseSchema } from "./capabilities";
import { Sheet } from "./Sheet";

/**
 * One tool's catalogue entry: what it is, what it takes, and whether it can change the machine.
 *
 * The sheet does not invoke anything. A tool is run by the page whose job it is, with the arguments
 * that page understands; the console never sends a tool's name and arbitrary JSON from here, so the
 * catalogue cannot become a REPL with the operator's authority.
 */
export function ToolSheet({ tool, onClose }: { tool: LiveTool; onClose: () => void }) {
  const summary = useMemo(() => summariseSchema(tool.inputSchema), [tool.inputSchema]);
  const destructive = tool.destructive;
  const source =
    tool.source === "builtin" ? "Built-in" : `Plugin · ${tool.source.replace(/^plugin:/, "")}`;

  return (
    <Sheet title={tool.name} onClose={onClose}>
      <div className="tool-detail">
        <p className="tool-meta">
          <span className="mono">{tool.capability}</span> · {source}
        </p>
        {tool.description && <p>{tool.description}</p>}
        <div className="chips">
          <span className="chip">{destructive ? "Destructive" : "Read-only"}</span>
        </div>
        <section className="schema" aria-label="Input schema">
          <h3>Input</h3>
          {summary.fields.length === 0 ? (
            <p className="hint">This tool takes no arguments.</p>
          ) : (
            <ul className="schema-fields">
              {summary.fields.map((field) => (
                <li key={field.name}>
                  <span className="mono">{field.name}</span>
                  <span className="chip">{field.type}</span>
                  {field.required && <span className="schema-required">Required</span>}
                  {field.description && <span className="schema-help">{field.description}</span>}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </Sheet>
  );
}
