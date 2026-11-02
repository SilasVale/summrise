import { useEffect, useMemo, useRef, useState } from "react";
import type { LiveTool } from "./api";
import {
  buildArguments,
  CALL_TIMEOUT_MS,
  formatResult,
  formArguments,
  initialForm,
  summariseSchema,
  TIMEOUT_SENTENCE,
  validateForm,
  type FormErrors,
  type FormState
} from "./capabilities";
import { copyText } from "./copy";
import { Sheet } from "./Sheet";
import { callTool, toolFailure, type Grant } from "./terminal";

type Outcome =
  | { kind: "ok"; text: string; pretty: string; parsed: boolean; lines: number; elapsed: number }
  | { kind: "error"; message: string; elapsed: number };

const RESULT_LINES = 400;

/**
 * One tool: what it is, what it takes, and a form generated from its schema so the operator can prove
 * the capability without writing a client.
 *
 * The form's states are the contract: blank optional is omitted, blank required is an inline error
 * that takes focus, booleans are Not sent/true/false, a nested schema is Raw JSON only, and the Raw
 * toggle refuses to switch back on invalid JSON. Run is pinned in the footer and is the only accent;
 * a destructive call asks once, inline, with the exact JSON that will run.
 */
export function ToolSheet({
  tool,
  grant,
  onClose,
  onActivity,
  timeoutMs = CALL_TIMEOUT_MS
}: {
  tool: LiveTool;
  grant: Grant;
  onClose: () => void;
  /// The success line links to the record, which is Activity.
  onActivity: () => void;
  /// A test seam: the state this guards is the sentence after the device's own two-minute limit.
  timeoutMs?: number;
}) {
  const summary = useMemo(() => summariseSchema(tool.inputSchema), [tool.inputSchema]);
  const [form, setForm] = useState<FormState>(() => initialForm(summary));
  const [errors, setErrors] = useState<FormErrors>({ fields: {}, raw: null, first: null });
  const [formError, setFormError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<Record<string, unknown> | null>(null);
  const [running, setRunning] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [copied, setCopied] = useState(false);
  const inputs = useRef<Record<string, HTMLElement | null>>({});
  const rawRef = useRef<HTMLTextAreaElement>(null);
  const runningRef = useRef(false);
  runningRef.current = running;

  // The copy button's "Copied" is a moment, not a state the sheet keeps.
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const change = (mutate: (next: FormState) => void) => {
    setForm((current) => {
      const next: FormState = {
        values: { ...current.values },
        booleans: { ...current.booleans },
        raw: current.raw,
        rawMode: current.rawMode
      };
      mutate(next);
      return next;
    });
    setConfirming(null);
    setFormError(null);
  };

  const setValue = (name: string, value: string) => {
    change((next) => {
      next.values[name] = value;
    });
    setErrors((current) => {
      if (!current.fields[name]) return current;
      const fields = { ...current.fields };
      delete fields[name];
      return { ...current, fields, first: current.first === name ? null : current.first };
    });
  };

  const setBoolean = (name: string, choice: string) => {
    change((next) => {
      next.booleans[name] = choice as FormState["booleans"][string];
    });
  };

  const setRaw = (raw: string) => {
    change((next) => {
      next.raw = raw;
    });
    setErrors((current) => (current.raw ? { ...current, raw: null, first: null } : current));
  };

  const serialise = () => {
    const result = formArguments(summary, form);
    if (!result.ok) {
      setFormError(result.message);
      return;
    }
    setFormError(null);
    setForm({
      ...form,
      raw: JSON.stringify(result.value, null, 2),
      rawMode: true
    });
    setConfirming(null);
  };

  const restoreForm = () => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(form.raw);
    } catch {
      setErrors({ fields: {}, raw: "Fix the JSON before switching back.", first: "$raw" });
      return;
    }
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
      setErrors({ fields: {}, raw: "The arguments must be a JSON object.", first: "$raw" });
      return;
    }
    const values: Record<string, string> = {};
    const booleans: Record<string, FormState["booleans"][string]> = {};
    for (const field of summary.fields) {
      const value = (parsed as Record<string, unknown>)[field.name];
      if (field.type === "boolean") {
        booleans[field.name] = value === true ? "true" : value === false ? "false" : "omit";
      } else {
        values[field.name] = value === undefined || value === null ? "" : String(value);
      }
    }
    setForm({ values, booleans, raw: form.raw, rawMode: false });
    setErrors({ fields: {}, raw: null, first: null });
    setFormError(null);
    setConfirming(null);
  };

  const toggleRaw = () => {
    if (summary.rawOnly) return;
    if (form.rawMode) restoreForm();
    else serialise();
  };

  const run = async (prepared?: Record<string, unknown>) => {
    if (runningRef.current) return;
    const errorsNow = validateForm(summary, form);
    setErrors(errorsNow);
    if (errorsNow.first) {
      // Focus the field the sentence names. A sheet that only colours a label leaves the operator
      // hunting for the one control that stopped the call.
      window.setTimeout(() => {
        if (errorsNow.first === "$raw") rawRef.current?.focus();
        else inputs.current[errorsNow.first ?? ""]?.focus();
      }, 0);
      return;
    }
    let args = prepared;
    if (!args) {
      const result = buildArguments(summary, form);
      if (!result.ok) {
        setFormError(result.message);
        return;
      }
      args = result.value;
    }
    if (tool.destructive && !prepared) {
      setConfirming(args);
      return;
    }

    setFormError(null);
    setOutcome(null);
    setShowAll(false);
    setRunning(true);
    setSeconds(0);
    const started = Date.now();
    const ticker = window.setInterval(() => setSeconds((value) => value + 1), 1000);
    let expired = false;
    const timer = window.setTimeout(() => {
      expired = true;
    }, timeoutMs);
    try {
      const answer = await Promise.race([
        callTool(grant, tool.name, args),
        new Promise<never>((_, reject) => {
          window.setTimeout(() => reject(new Error("__timeout__")), timeoutMs);
        })
      ]);
      const formatted = formatResult(answer);
      setOutcome({
        kind: "ok",
        text: answer,
        pretty: formatted.pretty,
        parsed: formatted.parsed,
        lines: formatted.lines,
        elapsed: Date.now() - started
      });
      setConfirming(null);
    } catch (cause) {
      const message =
        expired || (cause instanceof Error && cause.message === "__timeout__")
          ? TIMEOUT_SENTENCE
          : toolFailure(cause);
      setOutcome({ kind: "error", message, elapsed: Date.now() - started });
      setConfirming(null);
    } finally {
      window.clearInterval(ticker);
      window.clearTimeout(timer);
      setRunning(false);
      setSeconds(0);
    }
  };

  const destructive = tool.destructive;
  const source =
    tool.source === "builtin" ? "Built-in" : `Plugin · ${tool.source.replace(/^plugin:/, "")}`;
  const lines = outcome?.kind === "ok" ? outcome.pretty.split("\n") : [];
  const clipped = outcome?.kind === "ok" && outcome.lines > RESULT_LINES && !showAll;

  const footer = (
    <>
      <span className="sheet-status">
        {running ? `This can take up to two minutes.` : null}
      </span>
      <button
        type="button"
        className="primary"
        disabled={running}
        onClick={() => {
          if (confirming) void run(confirming);
          else void run();
        }}
      >
        {running
          ? `Running… ${seconds}s`
          : confirming
            ? "Run once"
            : "Run"}
      </button>
    </>
  );

  return (
    <Sheet title={tool.name} onClose={onClose} busy={running} footer={footer}>
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

        <section className="try" aria-label="Try it">
          <div className="try-head">
            <h3>Try it</h3>
            <button
              type="button"
              className="toggle"
              aria-pressed={form.rawMode}
              disabled={summary.rawOnly || running}
              onClick={toggleRaw}
            >
              Raw JSON
            </button>
          </div>
          {summary.rawOnly && summary.reason && <p className="hint">{summary.reason}</p>}

          {confirming && (
            <div className="confirm-run" role="alert">
              <p>This call can change this machine and cannot be undone. Run it?</p>
              <pre className="code">{JSON.stringify(confirming, null, 2)}</pre>
              <div className="actions">
                <button type="button" onClick={() => setConfirming(null)}>
                  Cancel
                </button>
              </div>
            </div>
          )}

          {form.rawMode ? (
            <label className="field">
              <span>Arguments (JSON)</span>
              <textarea
                ref={rawRef}
                value={form.raw}
                rows={10}
                spellCheck={false}
                aria-invalid={errors.raw ? true : undefined}
                aria-describedby={errors.raw ? "raw-error" : undefined}
                onChange={(event) => setRaw(event.target.value)}
              />
              {errors.raw && (
                <span className="error" id="raw-error" role="alert">
                  {errors.raw}
                </span>
              )}
            </label>
          ) : (
            <div className="form-grid">
              {summary.fields.map((field) => {
                const id = `arg-${field.name}`;
                const error = errors.fields[field.name];
                const describedBy = error ? `${id}-error` : undefined;
                const common = {
                  id,
                  "aria-invalid": error ? true : undefined,
                  "aria-describedby": describedBy
                } as const;
                return (
                  <label className="field" key={field.name} htmlFor={id}>
                    <span className="field-label">
                      {field.name}
                      {field.required && <span className="required"> required</span>}
                    </span>
                    {field.type === "boolean" ? (
                      <select
                        {...common}
                        ref={(node) => {
                          inputs.current[field.name] = node;
                        }}
                        value={form.booleans[field.name] ?? "omit"}
                        onChange={(event) => setBoolean(field.name, event.target.value)}
                      >
                        <option value="omit">Not sent</option>
                        <option value="true">true</option>
                        <option value="false">false</option>
                      </select>
                    ) : field.type === "enum" ? (
                      <select
                        {...common}
                        ref={(node) => {
                          inputs.current[field.name] = node;
                        }}
                        value={form.values[field.name] ?? ""}
                        onChange={(event) => setValue(field.name, event.target.value)}
                      >
                        <option value="">Not sent</option>
                        {field.enumValues.map((value) => (
                          <option key={value} value={value}>
                            {value}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <input
                        {...common}
                        ref={(node) => {
                          inputs.current[field.name] = node;
                        }}
                        type={field.type === "number" || field.type === "integer" ? "number" : "text"}
                        step={field.type === "integer" ? 1 : undefined}
                        value={form.values[field.name] ?? ""}
                        onChange={(event) => setValue(field.name, event.target.value)}
                      />
                    )}
                    {field.description && <span className="field-help">{field.description}</span>}
                    {error && (
                      <span className="error" id={`${id}-error`} role="alert">
                        {error}
                      </span>
                    )}
                  </label>
                );
              })}
            </div>
          )}
          {formError && <p className="error">{formError}</p>}

          {outcome && (
            <section className="result" aria-label="Result">
              <div className="result-head">
                <span className="hint">
                  {outcome.kind === "ok"
                    ? `Completed in ${outcome.elapsed} ms`
                    : `Failed after ${outcome.elapsed} ms`}
                </span>
                {outcome.kind === "ok" && (
                  <>
                    {outcome.lines > RESULT_LINES && (
                      <button type="button" className="link" onClick={() => setShowAll((value) => !value)}>
                        {showAll ? "Show less" : "Show all"}
                      </button>
                    )}
                    <button
                      type="button"
                      className="link"
                      onClick={() =>
                        void copyText(outcome.pretty).then((ok) => {
                          if (ok) setCopied(true);
                        })
                      }
                    >
                      {copied ? "Copied" : "Copy"}
                    </button>
                  </>
                )}
              </div>
              {outcome.kind === "ok" ? (
                <>
                  <pre className={"result-pre mono" + (showAll ? " expanded" : "")}>
                    {clipped ? `${lines.slice(0, RESULT_LINES).join("\n")}\n…` : outcome.pretty}
                  </pre>
                  <p className="hint">
                    Recorded in Activity.{" "}
                    <button type="button" className="link" onClick={onActivity}>
                      Open Activity
                    </button>
                  </p>
                </>
              ) : (
                <pre className="result-error mono" role="alert">
                  {outcome.message}
                </pre>
              )}
            </section>
          )}
        </section>
      </div>
    </Sheet>
  );
}
