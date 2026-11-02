/**
 * The tool picker the grant form and the permanent-token form share: one list, All/None, and the
 * selected count. The two forms differ in what they do with the choice, not in how it is made.
 */
export function ToolPicker({
  names,
  chosen,
  onChange
}: {
  names: string[];
  chosen: Set<string>;
  onChange: (next: Set<string>) => void;
}) {
  const toggle = (name: string) => {
    const next = new Set(chosen);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    onChange(next);
  };

  return (
    <fieldset className="tools">
      <legend>
        Tools ({chosen.size} of {names.length})
        <button type="button" className="mini" onClick={() => onChange(new Set(names))}>
          All
        </button>
        <button type="button" className="mini" onClick={() => onChange(new Set())}>
          None
        </button>
      </legend>
      <div className="tool-grid">
        {names.map((name) => (
          <label key={name} className="check mono">
            <input type="checkbox" checked={chosen.has(name)} onChange={() => toggle(name)} />
            {name}
          </label>
        ))}
      </div>
    </fieldset>
  );
}
