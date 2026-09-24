import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "../i18n.ts";
import { useToast } from "../contexts/ToastContext.tsx";
import { api, ApiError } from "../api/client.ts";
import { PageHeader, Badge, CopyButton } from "../components/ui.tsx";
// ONE OWNER for the key vocabulary: it used to be an array literal here and another one in
// Overview.tsx, and the two had already drifted (this file listed nine, the tile eight).
import { KEY_NAMES } from "../lib/keyNames.ts";

interface KeyInfo {
  configured: boolean;
  masked: string;
  /** WHICH CREDENTIAL SERVES THIS CHANNEL — the gateway's own answer, derived from
   *  the same channel table its request path reads, not a guess made here.
   *  `configured` says the user stored a key; `source` says what a request will
   *  actually be spent on, which is the question this page exists to answer. */
  source?: "user" | "deployment" | "none";
}

// Full env names → the i18n prefixes ("key.<prefix>.backend/.hint"). The old
// lowercase-suffix guess produced raw keys on the page ("key.deepseek.backend").
const KEY_I18N_PREFIX: Record<string, string> = {
  DEEPSEEK_API_KEY: "ds",
  OPENCODE_GO_API_KEY: "og",
  QWEN_API_KEY: "qw",
  OPENROUTER_API_KEY: "or",
  NVAPI_KEY: "nv",
  GMI_API_KEY: "gmi",
  CMD_API_KEY: "cmd",
  AMD_API_KEY: "amd",
  R4_API_KEY: "r4",
};

function getKeyShortName(name: string) {
  return KEY_I18N_PREFIX[name] || name.replace("_API_KEY", "").toLowerCase();
}

export default function Keys() {
  const { t } = useTranslation();
  const { toast } = useToast();
  const [keys, setKeys] = useState<Record<string, KeyInfo | undefined>>({});
  // A FAILED READ IS NOT AN EMPTY ACCOUNT. `keys` starts empty and stays empty when `/api/me` fails,
  // and an empty object renders every card as "not configured" — nine confident claims about nine
  // credentials, on the page whose whole job is those credentials. This flag is what tells the two
  // states apart; a successful read clears it, so it cannot latch.
  const [loadFailed, setLoadFailed] = useState(false);
  const [editingName, setEditingName] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [resultBox, setResultBox] = useState<{ name: string; ok: boolean; msg: string } | null>(
    null,
  );
  const [testing, setTesting] = useState<string | null>(null);
  const [usageLoading, setUsageLoading] = useState<string | null>(null);

  const loadKeys = useCallback(async () => {
    try {
      const me = await api.me();
      setKeys(me.keys || {});
      setLoadFailed(false);
    } catch {
      // Was `/* noop */`, which is exactly how a failure became a claim about nine credentials.
      setLoadFailed(true);
    }
  }, []);

  useEffect(() => {
    loadKeys();
  }, [loadKeys]);

  const handleSave = async (name: string) => {
    if (!editValue.trim()) {
      setResultBox({ name, ok: false, msg: t("key.emptyValue") });
      return;
    }
    try {
      await api.saveKey(name, editValue.trim());
      setEditingName(null);
      setEditValue("");
      toast(`${t("key.saved")} ${name}`);
      await loadKeys();
    } catch (err) {
      setResultBox({
        name,
        ok: false,
        msg: err instanceof ApiError ? err.message : t("key.saveFail"),
      });
    }
  };

  const handleTest = async (name: string) => {
    setTesting(name);
    try {
      const data = await api.testKey(name);
      setResultBox({
        name,
        ok: data.ok,
        msg: data.ok
          ? t("key.testOk", { status: String(data.status || 200) }) +
            (data.detail ? ` · ${data.detail}` : "")
          : t("key.testFail", { detail: data.detail || "…" }),
      });
    } catch (err) {
      const msg = err instanceof ApiError ? err.message : "…";
      setResultBox({ name, ok: false, msg: t("key.testFail", { detail: msg }) });
    }
    setTesting(null);
  };

  const handleUsage = async (name: string) => {
    setUsageLoading(name);
    try {
      const data = await api.usageKey(name);
      if (!data.ok) {
        setResultBox({ name, ok: false, msg: data.detail || t("key.usageFail") });
      } else {
        const money = (v: number | undefined) =>
          typeof v === "number" && Number.isFinite(v)
            ? `$${v.toFixed(4)}`
            : t("key.usageUnavailable");
        const parts: string[] = [];
        if (data.label) parts.push(`${t("key.usageAccount")}: ${data.label}`);
        // OpenCode Go multi-window response
        if (data.windows && typeof data.windows === "object") {
          // The window labels come from i18n like everything else: they were CJK
          // literals in the source, so the ENGLISH console showed 周 / 月.
          const winLabels: Record<string, string> = {
            "5h": "5h",
            weekly: t("key.windowWeekly"),
            monthly: t("key.windowMonthly"),
          };
          for (const [wk, wv] of Object.entries(data.windows) as [string, any][]) {
            const label = winLabels[wk] || wk;
            const pct =
              typeof wv.used === "number" && typeof wv.limit === "number" && wv.limit > 0
                ? ` ${Math.round((wv.used / wv.limit) * 100)}%`
                : "";
            const remain =
              typeof wv.remaining === "number"
                ? ` · ${t("key.usageRemaining")}: ${money(wv.remaining)}`
                : "";
            const reset = wv.resetAt ? ` · ${wv.resetAt}` : "";
            parts.push(
              `${label}: ${money(wv.used)} / ${wv.limit === null ? t("key.usageUnlimited") : money(wv.limit)}${pct}${remain}${reset}`,
            );
          }
        } else {
          // Single-value response (OpenRouter / flat OpenCode Go)
          parts.push(`${t("key.usageUsed")}: ${money(data.usage)}`);
          parts.push(
            `${t("key.usageLimit")}: ${data.limit === null ? t("key.usageUnlimited") : money(data.limit)}`,
          );
          if (typeof data.usage === "number" && typeof data.limit === "number")
            parts.push(
              `${t("key.usageRemaining")}: ${money(Math.max(0, data.limit - data.usage))}`,
            );
          if (typeof data.balance === "number")
            parts.push(`${t("key.balanceLabel")}: ${money(data.balance)}`);
        }
        if (data.rateLimit?.limit != null)
          parts.push(
            `${t("key.usageRateLimit")}: ${data.rateLimit.limit}${data.rateLimit.interval ? `/${data.rateLimit.interval}` : ""}`,
          );
        setResultBox({ name, ok: true, msg: parts.join(" · ") });
      }
    } catch (err) {
      setResultBox({
        name,
        ok: false,
        msg: err instanceof ApiError ? err.message : t("key.usageFail"),
      });
    }
    setUsageLoading(null);
  };

  const handleClear = async (name: string) => {
    if (!confirm(t("key.clearConfirm", { name }))) return;
    try {
      await api.deleteKey(name);
      toast(`${t("key.cleared")} ${name}`);
      await loadKeys();
    } catch (err) {
      toast(err instanceof ApiError ? err.message : t("key.saveFail"), true);
    }
  };

  return (
    <div>
      <PageHeader
        title={t("keys.title")}
        description={t("keys.lede")}
        // THE PAGE ANSWERS ITS OWN QUESTION, in the same slot and with the same idiom
        // the Models page uses for its count. You come here to fix keys, so the first
        // thing it should say is how many are missing — the Overview tile knew, and the
        // page you click through to did not. (That tile hard-coded EIGHT names and this
        // page nine, so it said "0/8" about a page reading "0 / 9"; both now read one
        // list, `lib/keyNames.ts`, and byok.test.mjs forbids either view from keeping
        // its own copy again.)
        actions={
          // TWO NUMBERS, because since the env fallback they answer different questions:
          // "yours" is what this user pays for, "deployment" is what the worker serves on
          // their behalf. One number called the second group missing while it was working.
          //
          // AND WHEN THE READ FAILED, NO NUMBERS AT ALL — they would be counts of an empty object,
          // which is indistinguishable from an account with nothing configured. The state is known
          // (`loadFailed`), so it is stated where the false count used to be.
          loadFailed ? (
            <Badge tone="muted">{t("keys.loadFail")}</Badge>
          ) : (
            <Badge tone="muted">
              {t("keys.summary", {
                done: String(KEY_NAMES.filter((n) => keys[n]?.source === "user").length),
                total: String(KEY_NAMES.length),
                deployment: String(KEY_NAMES.filter((n) => keys[n]?.source === "deployment").length),
              })}
            </Badge>
          )
        }
      />
      <div className="cards">
        {KEY_NAMES.map((name) => {
          const info = keys[name];
          const configured = !!(info && info.configured);
          const source = info?.source ?? (configured ? "user" : "none");
          const shortName = getKeyShortName(name);
          const backend = t(`key.${shortName}.backend` as any);
          const hint = t(`key.${shortName}.hint` as any);
          const isEditing = editingName === name;
          const result = resultBox?.name === name ? resultBox : null;

          return (
            <div className="key-card" key={name}>
              <div className="key-card-top">
                <div>
                  <div className="key-card-name">{name}</div>
                  <div className="key-card-desc">
                    {backend} · {hint}
                  </div>
                </div>
                {/* THREE STATES, NOT TWO. "Configured / Not configured" was the whole
                    vocabulary, and after the fallback landed it was wrong in the case that
                    matters most: a channel the deployment serves read as "not configured"
                    on a page whose job is to explain why requests do or do not work. */}
                <Badge
                  tone={source === "user" ? "success" : source === "deployment" ? "info" : "muted"}
                >
                  {source === "user"
                    ? t("key.sourceUser")
                    : source === "deployment"
                      ? t("key.sourceDeployment")
                      : t("key.sourceNone")}
                </Badge>
              </div>

              {/* ONLY WHEN THERE IS A VALUE. The badge above already carries the
                  state, so an unconfigured card printed "Not configured" twice — once
                  as a badge and once inside a box styled to look like an input — and
                  eight of those boxes were the heaviest thing on the page. Nothing to
                  show, nothing rendered: the action below is the whole card. */}
              {configured && (
                <div className="row">
                  <code className="token" style={{ flex: 1 }}>
                    {info?.masked}
                  </code>
                  <CopyButton
                    text={info?.masked || ""}
                    // The list carries only the masked display value — fetch the
                    // FULL key at click time (session-gated reveal) so the clipboard
                    // gets the real credential, not the mask. Failures (reveal or
                    // clipboard) toast once via onFailed.
                    getText={async () => {
                      const { value } = await api.revealKey(name);
                      return value;
                    }}
                    onFailed={() => toast(t("key.revealFail"), true)}
                    small
                  />
                </div>
              )}

              {/* THE DEPLOYMENT'S KEY IS NOT THIS USER'S TO COPY, so no masked box and no
                  reveal button — just the sentence that says who is paying and how to change
                  it. Rendering a mask here would invite a copy of something the page does not
                  have, and offering "reveal" would leak a Worker secret to any session. */}
              {source === "deployment" && (
                <p className="key-card-desc">
                  {t("key.deploymentServed", { backend: backend as any })}
                </p>
              )}

              <div className="key-card-actions">
                {/* THE PRIMARY ACTION IS "SET KEY", NOT "EDIT" ×8. A page whose job
                    is to fill in what is missing cannot have eight equally loud solid
                    buttons, none of which is the thing to do next; on a configured
                    card the same control steps back to a ghost. */}
                <button
                  className={`btn ${configured ? "btn-ghost" : "btn-primary"} btn-mini`}
                  onClick={() => {
                    setEditingName(name);
                    setEditValue("");
                    setResultBox(null);
                  }}
                >
                  {configured ? t("btn.edit") : t("btn.setKey")}
                </button>
                <button
                  className="btn btn-ghost btn-mini"
                  disabled={testing === name}
                  onClick={() => handleTest(name)}
                >
                  {testing === name ? t("btn.testing") : t("btn.test")}
                </button>
                {(name === "OPENROUTER_API_KEY" ||
                  name === "OPENCODE_GO_API_KEY" ||
                  name === "AMD_API_KEY") && (
                  <button
                    className="btn btn-ghost btn-mini"
                    disabled={usageLoading === name}
                    onClick={() => handleUsage(name)}
                  >
                    {usageLoading === name ? t("btn.usageLoading") : t("btn.usage")}
                  </button>
                )}
                <button className="btn btn-danger btn-mini" onClick={() => handleClear(name)}>
                  {t("btn.clear")}
                </button>
              </div>

              {isEditing && (
                <div className="input-row">
                  <input
                    className="form-input"
                    type="text"
                    placeholder="sk-…"
                    autoComplete="off"
                    value={editValue}
                    onChange={(e) => setEditValue(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleSave(name)}
                    autoFocus
                  />
                  <button className="btn btn-primary btn-mini" onClick={() => handleSave(name)}>
                    {t("btn.save")}
                  </button>
                  <button
                    className="btn btn-ghost btn-mini"
                    onClick={() => {
                      setEditingName(null);
                      setEditValue("");
                    }}
                  >
                    {t("btn.cancel")}
                  </button>
                </div>
              )}

              {result && (
                <div className={`test-result ${result.ok ? "ok" : "err"}`}>{result.msg}</div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
