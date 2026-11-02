/**
 * A tag list and its add box.
 *
 * The edit form and the create dialog collect tags the same way: chips that remove one tag, and an
 * input whose Enter or **Add tag** appends the draft. The page owns the state and the device call;
 * this component is the shape both surfaces share.
 */
export function TagEditor({
  tags,
  draft,
  onDraft,
  onAdd,
  onRemove,
  inputId,
  datalistId
}: {
  tags: readonly string[];
  draft: string;
  onDraft: (value: string) => void;
  onAdd: () => void;
  onRemove: (index: number) => void;
  inputId: string;
  datalistId?: string;
}) {
  return (
    <>
      <label htmlFor={inputId}>Tags</label>
      {tags.length === 0 ? (
        <p className="hint">No tags.</p>
      ) : (
        <div className="memory-tags">
          {tags.map((value, index) => (
            <button
              type="button"
              className="chip chip-tag"
              key={`${value}-${index}`}
              title={`Remove ${value}`}
              onClick={() => onRemove(index)}
            >
              {value} ×
            </button>
          ))}
        </div>
      )}
      <div className="memory-add-tag">
        <input
          id={inputId}
          value={draft}
          onChange={(event) => onDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              onAdd();
            }
          }}
          list={datalistId}
          placeholder="a tag; Enter adds it"
        />
        <button
          type="button"
          className="mini"
          disabled={draft.trim() === ""}
          onClick={onAdd}
        >
          Add tag
        </button>
      </div>
    </>
  );
}
