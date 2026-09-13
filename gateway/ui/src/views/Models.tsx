// Models — the catalogue this gateway ADVERTISES, as a PROVIDER LIST.
//
// WHY THIS PAGE EXISTS, AND WHY IT LOOKS LIKE THIS. `/api/admin/public` returns
// `ROUTE_INFO`: every channel with its `prefix`, `backend`, `desc` and a `models`
// list DERIVED from `MODEL_REGISTRY` (so it cannot drift from what `/v1/models`
// serves). The console used to render that as one flat card per channel with a
// chip per model, plus a permanent "add a model" form in the page's most valuable
// slot — a form only an admin sees and almost nobody uses, while the page's own
// lede says the point is to PICK a route. The chrome was upside down.
//
// The layout is now the one the DeepSeek Harness uses for the same job, because
// the shape of the data is the same: a provider (prefix + endpoint + protocol +
// credential) OWNS a list of models. So:
//   * one ROW per provider — identity, a [Custom] tag, a credential dot, Edit,
//     and Delete only where deleting is real (a custom provider);
//   * the model list lives INSIDE the provider's editor, opened in place, one at
//     a time;
//   * adding is a trailing dashed button, not a form that owns the top of the page.
//
// EVERY ROUTE HERE ALREADY EXISTED. Providers come from `/api/admin/providers`
// (keyReady is the dot: the server reduces the credential to a mask, never
// returns it); models are added/disabled/deleted through `/api/admin/models`; the
// route is set through `/api/me/route`. The ONE thing this page still cannot do
// is ask an upstream what it serves and adopt the answer — `scripts/model-drift.mjs`
// can, but it is an ops tool, not a route, and giving the worker that job means a
// new outbound-request surface that deserves its own review.
import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "../i18n.ts";
import { useToast } from "../contexts/ToastContext.tsx";
import { useAuth } from "../contexts/AuthContext.tsx";
import {
  api,
  ApiError,
  type RouteInfo,
  type HealthChannel,
  type ProviderView,
  type ProbeResult,
  type ModelFacets,
} from "../api/client.ts";
import { PageHeader, Badge } from "../components/ui.tsx";

/** The lane colour for a channel prefix — the same mapping the Routes page uses,
 *  so a channel looks the same wherever it appears. */
function laneClass(prefix: string): string {
  const p = prefix.replace(/\/$/, "");
  if (p === "og") return "lane-og";
  if (p === "ds") return "lane-ds";
  if (p === "or") return "lane-or";
  if (p === "qw") return "lane-qw";
  if (p === "nv") return "lane-nv";
  if (p === "gmi") return "lane-gmi";
  if (p === "cm") return "lane-cm";
  if (p === "amd") return "lane-amd";
  return "lane-def";
}

export default function ModelsView() {
  const { t } = useTranslation();
  const { toast } = useToast();
  const { user } = useAuth();
  const [routes, setRoutes] = useState<RouteInfo[]>([]);
  // The PREFIXED catalogue (== /v1/models). Models are rendered from this, never
  // from `routes[].models`, which is bare and would set the wrong channel.
  const [allModels, setAllModels] = useState<string[]>([]);
  const [health, setHealth] = useState<HealthChannel[]>([]);
  const [providers, setProviders] = useState<ProviderView[]>([]);
  // What THIS build can speak. Empty until the server says — a hardcoded fallback
  // here is exactly how the form ended up offering a protocol the server rejects.
  const [apis, setApis] = useState<string[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [switching, setSwitching] = useState<string | null>(null);
  // Admin-only: which models this console owns vs which are built-in-but-off.
  // Non-admins never get the data — the routes are admin-gated too.
  const [custom, setCustom] = useState<string[]>([]);
  const [disabled, setDisabled] = useState<string[]>([]);
  // What a console-owned model DECLARES. Declaring without ever showing it again is how
  // a value silently disappears from an operator's view of their own catalogue.
  const [facets, setFacets] = useState<Record<string, ModelFacets>>({});
  const [busy, setBusy] = useState<string | null>(null);
  // ONE OPEN EDITOR AT A TIME, keyed by prefix. Two open editors would mean two
  // half-typed drafts and two save paths on one screen; the harness models the
  // same rule for the same reason.
  const [open, setOpen] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState({
    id: "",
    wire: "",
    usEgress: false,
    search: false,
    // The form-owned facets. Collapsed by default because most models declare none —
    // DSH folds them behind the same word for the same reason.
    name: "",
    contextWindow: "",
    maxTokens: "",
    // Typed as the union (plus "") so the empty option is "no default" rather than
    // an arbitrary string the API would have to reject.
    reasoningEffort: "" as "" | "low" | "medium" | "high" | "max",
  });
  const [advanced, setAdvanced] = useState(false);
  const [newProvider, setNewProvider] = useState({ prefix: "", label: "", baseURL: "", api: "", apiKey: "" });
  const [newModel, setNewModel] = useState("");
  // The probe result is keyed by prefix so a stale answer cannot be shown against a
  // row the user has since opened.
  const [probe, setProbe] = useState<(ProbeResult & { prefix: string }) | null>(null);
  const [probing, setProbing] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [info, health, route] = await Promise.all([
        api.getPublicRoutes().catch(() => null),
        api.getHealth().catch(() => null),
        api.getRoute().catch(() => null),
      ]);
      // A catalogue that could not be read must SAY SO: an empty page would claim
      // the gateway advertises nothing, which is a different fact.
      if (info?.models?.length) {
        setAllModels(info.models);
        setRoutes(info.routes || []);
      } else setFailed(true);
      // Best-effort: the page is fully usable read-only, so a 403 here means
      // "no admin controls" rather than an error worth showing.
      void api
        .getModelState()
        .then((st) => {
          setCustom(st.custom || []);
          setDisabled(st.disabled || []);
          setFacets(st.facets || {});
        })
        .catch(() => {});
      void api
        .getProviders()
        .then((r) => {
          setProviders(r.providers || []);
          setApis(r.apis || []);
        })
        .catch(() => {});
      if (health?.channels) setHealth(health.channels);
      if (route?.effective) setCurrent(route.effective);
      else if (route?.model) setCurrent(route.model);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const switchTo = useCallback(
    async (model: string) => {
      setSwitching(model);
      try {
        await api.setRoute(model);
        setCurrent(model);
        toast(t("models.switched"));
      } catch {
        toast(t("route.fail"));
      } finally {
        setSwitching(null);
      }
    },
    [t, toast],
  );

  const isAdmin = user?.role === "admin";

  /** Delete a model this console owns, or disable a built-in — the server decides
   *  which, from the id; the console only asks for the action the user chose. */
  const removeModel = useCallback(
    async (id: string) => {
      const builtIn = !custom.includes(id);
      if (!confirm(builtIn ? t("models.disableConfirm", { id }) : t("models.deleteConfirm", { id }))) return;
      setBusy(id);
      try {
        await api.deleteModel(id);
        toast(builtIn ? t("models.disabled") : t("models.deleted"));
        await load();
      } catch (err) {
        toast(err instanceof ApiError ? err.message : t("route.fail"), true);
      } finally {
        setBusy(null);
      }
    },
    [custom, load, t, toast],
  );

  const enableModel = useCallback(
    async (id: string) => {
      setBusy(id);
      try {
        await api.enableModel(id);
        toast(t("models.enabled"));
        await load();
      } catch (err) {
        toast(err instanceof ApiError ? err.message : t("route.fail"), true);
      } finally {
        setBusy(null);
      }
    },
    [load, t, toast],
  );

  /** Add a model to ONE provider. The prefix is the row's, not a dropdown: the
   *  context is the provider you opened, so an id can no longer be filed under a
   *  channel the user did not mean. */
  const submitModel = useCallback(
    async (prefix: string) => {
      const id = prefix + draft.id.trim();
      if (!draft.id.trim()) return;
      setAdding(true);
      try {
        // Numbers travel as numbers: the server refuses a string, and `""` means
        // "not declared" rather than zero — which is exactly the distinction its
        // validation message is written around.
        const ctx = draft.contextWindow.trim() ? Number(draft.contextWindow) : undefined;
        const max = draft.maxTokens.trim() ? Number(draft.maxTokens) : undefined;
        await api.addModel({
          id,
          ...(draft.name.trim() ? { name: draft.name.trim() } : {}),
          // A DEFAULT, not a policy: the client's own reasoning always wins, which is
          // the only reason a form may set it at all.
          ...(draft.reasoningEffort ? { reasoningEffort: draft.reasoningEffort } : {}),
          ...(ctx !== undefined ? { contextWindow: ctx } : {}),
          ...(max !== undefined ? { maxTokens: max } : {}),
          ...(draft.wire.trim() ? { wire: draft.wire.trim() } : {}),
          ...(draft.usEgress ? { usEgress: true } : {}),
          ...(draft.search ? { search: true } : {}),
        });
        toast(t("models.added"));
        setDraft({
          id: "",
          wire: "",
          usEgress: false,
          search: false,
          name: "",
          contextWindow: "",
          maxTokens: "",
          reasoningEffort: "",
        });
        await load();
      } catch (err) {
        toast(err instanceof ApiError ? err.message : t("route.fail"), true);
      } finally {
        setAdding(false);
      }
    },
    [draft, load, t, toast],
  );

  /** Declare a custom provider: prefix + endpoint + protocol + key. Upsert by
   *  prefix, which is how an operator edits the record they already own. */
  const submitProvider = useCallback(async () => {
    const bare = newProvider.prefix.trim().replace(/\/+$/, "");
    if (!bare || !newProvider.baseURL.trim()) return;
    setAdding(true);
    try {
      await api.addProvider({
        prefix: bare,
        label: newProvider.label.trim() || bare,
        baseURL: newProvider.baseURL.trim(),
        api: newProvider.api,
        models: [],
        ...(newProvider.apiKey.trim() ? { apiKey: newProvider.apiKey.trim() } : {}),
      });
      toast(t("models.added"));
      setNewProvider({ prefix: "", label: "", baseURL: "", api: apis[0] || "", apiKey: "" });
      setOpen(bare + "/");
      await load();
    } catch (err) {
      toast(err instanceof ApiError ? err.message : t("route.fail"), true);
    } finally {
      setAdding(false);
    }
  }, [newProvider, load, t, toast]);

  /** Give a custom provider one more model: the record is the owner of its list,
   *  so this re-posts it with the model appended (the server upserts by prefix). */
  const addModelToProvider = useCallback(
    async (p: ProviderView) => {
      const id = newModel.trim().replace(/^\/+/, "");
      if (!id) return;
      setAdding(true);
      try {
        await api.addProvider({
          prefix: p.prefix,
          label: p.label,
          baseURL: p.baseURL,
          api: p.api,
          ...(p.keyEnv ? { apiKeyEnv: p.keyEnv } : {}),
          models: [...p.models.map((m) => ({ id: m.id, name: m.name, contextWindow: m.contextWindow, maxTokens: m.maxTokens, input: m.input })), { id }],
        });
        setNewModel("");
        toast(t("models.added"));
        await load();
      } catch (err) {
        toast(err instanceof ApiError ? err.message : t("route.fail"), true);
      } finally {
        setAdding(false);
      }
    },
    [newModel, load, t, toast],
  );

  const removeProvider = useCallback(
    async (prefix: string) => {
      if (!confirm(t("models.removeProviderConfirm", { prefix }))) return;
      setBusy(prefix);
      try {
        await api.deleteProvider(prefix);
        toast(t("models.deleted"));
        await load();
      } catch (err) {
        toast(err instanceof ApiError ? err.message : t("route.fail"), true);
      } finally {
        setBusy(null);
      }
    },
    [load, t, toast],
  );

  /**
   * The models belonging to one provider.
   *
   * THE DEFAULT CHANNEL CANNOT BE DERIVED FROM THE PREFIXED CATALOGUE. `"none"` is
   * the server's sentinel for "no prefix -> Command Code, model name passed through
   * as-is", and every advertised id in that catalogue is PREFIXED — so filtering it
   * for "ids matching no known prefix" always yields NOTHING, and that row used to
   * render 0 while its own server entry listed models.
   *
   * For that row the server's `routes[].models` ARE the right source, and the right
   * FORM too (bare names, which is what routes there). For every other row
   * `routes[].models` is the trap: bare where the catalogue is prefixed, which is
   * what once set the wrong channel.
   */
  const modelsFor = (prefix: string, fallback: string[]): string[] =>
    prefix && prefix !== "none" ? allModels.filter((m) => m.startsWith(prefix)) : fallback;
  // Health is reported per channel PREFIX; the catalogue is the authority on which
  // channels exist, so an id with no health entry is "not checked", not "down".
  const healthFor = (prefix: string) =>
    health.find((h) => h.id === prefix || h.id === prefix.replace(/\/$/, ""));

  /** Ask the upstream what it serves. `checked:false` carries a REASON and is shown
   *  as such — the server refuses to turn "could not look" into "offers nothing", and
   *  the panel must not undo that by rendering an empty list. */
  const runProbe = useCallback(
    async (prefix: string) => {
      setProbing(prefix);
      setProbe(null);
      try {
        setProbe({ ...(await api.probeModels(prefix)), prefix });
      } catch (err) {
        toast(err instanceof ApiError ? err.message : t("route.fail"), true);
      } finally {
        setProbing(null);
      }
    },
    [t, toast],
  );

  /** Adopt one discovered model. The probe returns PREFIXED ids, so for a built-in
   *  channel this is a direct call to the add route. A custom provider OWNS its list,
   *  so there the record is re-posted with the model appended — the same path the
   *  panel's own "add a model" uses, which is why adopting cannot invent a second
   *  write shape. */
  const adoptModel = useCallback(
    async (pv: ProviderView | undefined, qid: string) => {
      setAdding(true);
      try {
        if (pv) {
          const bare = qid.slice(pv.prefix.replace(/\/+$/, "").length + 1);
          await api.addProvider({
            prefix: pv.prefix,
            label: pv.label,
            baseURL: pv.baseURL,
            api: pv.api,
            ...(pv.keyEnv ? { apiKeyEnv: pv.keyEnv } : {}),
            models: [
              ...pv.models.map((m) => ({
                id: m.id,
                name: m.name,
                contextWindow: m.contextWindow,
                maxTokens: m.maxTokens,
                input: m.input,
              })),
              { id: bare },
            ],
          });
        } else {
          await api.addModel({ id: qid });
        }
        toast(t("models.added"));
        await load();
      } catch (err) {
        toast(err instanceof ApiError ? err.message : t("route.fail"), true);
      } finally {
        setAdding(false);
      }
    },
    [load, t, toast],
  );

  const total = allModels.length;
  /** The declared facets of a model, from whichever store owns the declaration. */
  const facetsOf = (id: string, pv?: ProviderView): ModelFacets | null => {
    const own = facets[id];
    if (own) return own;
    const bare = id.slice(id.indexOf("/") + 1);
    const m = pv?.models.find((x) => x.id === bare);
    if (!m) return null;
    return {
      name: m.name,
      contextWindow: m.contextWindow,
      maxTokens: m.maxTokens,
      reasoningEffort: m.reasoningEffort,
    };
  };

  const providerFor = (prefix: string) =>
    providers.find((p) => p.prefix.replace(/\/$/, "") === prefix.replace(/\/$/, ""));

  return (
    <>
      <PageHeader
        title={t("nav.models")}
        description={t("models.lede")}
        actions={<Badge tone="muted">{loading ? t("loading") : `${total} ${t("models.count")}`}</Badge>}
      />

      {failed && (
        <div className="banner-error">
          <span>{t("models.unavailable")}</span>
          <button className="btn btn-ghost btn-mini" onClick={() => void load()}>
            {t("devices.retry")}
          </button>
        </div>
      )}

      <ul className="prov-list">
        {routes.map((r) => {
          const prefix = r.prefix || "none";
          const pv = providerFor(prefix);
          const isCustom = !!pv;
          const models = modelsFor(prefix, r.models || []);
          const h = healthFor(prefix);
          // THE DOT ANSWERS "CAN I USE THIS?" — one signal, two sources, and the
          // title says which. A built-in channel's credential is the USER's BYOK
          // key, which this page does not fetch, so its dot reports the channel's
          // own reachability (the same fact the Routes page shows). A custom
          // provider's is the record's key, which the server already resolved.
          const ready = isCustom ? pv!.keyReady : h?.ok !== false;
          const dotLabel = isCustom
            ? pv!.keyReady
              ? t("models.credReady")
              : t("models.credMissing")
            : h?.ok === false
              ? t("models.channelDown")
              : t("models.channelUp");
          const isOpen = open === prefix;
          const mine = disabled.filter((d) => d.startsWith(prefix));
          return (
            <li key={prefix} className={`prov-row${isOpen ? " open" : ""}`}>
              <div className="prov-head">
                <span className="prov-ident">
                  <span className={`prov-lane ${laneClass(prefix)}`} aria-hidden="true" />
                  <span className="prov-name">{r.backend || prefix}</span>
                  {isCustom && <span className="prov-tag">{t("models.customTag")}</span>}
                  <span
                    className={`prov-dot${ready ? " ok" : " missing"}`}
                    role="img"
                    aria-label={dotLabel}
                    title={dotLabel}
                  />
                  <span className="prov-count">
                    {t("models.providerModels", { n: String(models.length) })}
                  </span>
                </span>
                <span className="prov-actions">
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    aria-expanded={isOpen}
                    onClick={() => {
                      setOpen(isOpen ? null : prefix);
                      setDraft({
          id: "",
          wire: "",
          usEgress: false,
          search: false,
          name: "",
          contextWindow: "",
          maxTokens: "",
          reasoningEffort: "",
        });
                      setNewModel("");
                    }}
                  >
                    {isOpen ? t("models.editorClose") : t("btn.edit")}
                  </button>
                  {isCustom && isAdmin && (
                    <button
                      type="button"
                      className="btn btn-danger-text btn-sm"
                      disabled={busy === prefix}
                      onClick={() => void removeProvider(prefix)}
                    >
                      {t("models.removeProvider")}
                    </button>
                  )}
                </span>
              </div>

              {isOpen && (
                <div className="prov-body">
                  <p className="prov-desc">
                    {isCustom
                      ? `${pv!.baseURL} · ${pv!.api}${pv!.keyMasked ? ` · ${pv!.keyMasked}` : ""}`
                      : r.desc}
                  </p>

                  {models.length === 0 ? (
                    <p className="muted">{t("models.noModels")}</p>
                  ) : (
                    <ul className="prov-models">
                      {models.map((id) => (
                        <li key={id} className="prov-model">
                          <code className="prov-model-id">{id}</code>
                          {/* What the model declares, in the row that owns it. Only the
                              facets a model MAY own appear here — never wire/us-egress,
                              which are routing semantics and live in the registry. */}
                          {(() => {
                            const f = facetsOf(id, pv);
                            if (!f) return null;
                            const bits = [
                              f.name,
                              f.contextWindow ? `${f.contextWindow} ctx` : "",
                              f.maxTokens ? `${f.maxTokens} out` : "",
                              f.reasoningEffort ? `effort ${f.reasoningEffort}` : "",
                            ].filter(Boolean);
                            return bits.length ? (
                              <span className="prov-model-facets">{bits.join(" · ")}</span>
                            ) : null;
                          })()}
                          {current === id && <Badge tone="success">{t("models.currentRoute")}</Badge>}
                          <span className="prov-model-actions">
                            <button
                              type="button"
                              className="btn btn-ghost btn-mini"
                              disabled={current === id || switching !== null}
                              title={current === id ? t("models.isCurrent") : t("models.setCurrent")}
                              onClick={() => void switchTo(id)}
                            >
                              {t("models.setCurrent")}
                            </button>
                            {isAdmin && (
                              <button
                                type="button"
                                className="btn btn-danger btn-mini"
                                disabled={busy !== null}
                                title={custom.includes(id) ? t("models.delete") : t("models.disable")}
                                onClick={() => void removeModel(id)}
                              >
                                {custom.includes(id) ? t("models.delete") : t("models.disable")}
                              </button>
                            )}
                          </span>
                        </li>
                      ))}
                    </ul>
                  )}

                  {isAdmin && (
                    <div className="prov-probe">
                      <button
                        type="button"
                        className="btn btn-secondary btn-sm"
                        disabled={probing !== null}
                        onClick={() => void runProbe(prefix)}
                      >
                        {probing === prefix ? t("models.probing") : t("models.probe")}
                      </button>
                      {probe?.prefix === prefix &&
                        (probe.checked ? (
                          <div className="prov-probe-out">
                            {(probe.notAdvertised ?? []).length === 0 ? (
                              <span className="muted">{t("models.probeNone", { prefix })}</span>
                            ) : (
                              <>
                                <span className="prov-probe-label">{t("models.probeOffered")}</span>
                                <ul className="prov-probe-list">
                                  {(probe.notAdvertised ?? []).map((qid) => (
                                    <li key={qid}>
                                      <code className="prov-model-id">{qid}</code>
                                      <button
                                        type="button"
                                        className="btn btn-ghost btn-mini"
                                        disabled={adding}
                                        onClick={() => void adoptModel(pv, qid)}
                                      >
                                        {t("models.probeAdopt")}
                                      </button>
                                    </li>
                                  ))}
                                </ul>
                              </>
                            )}
                            {/* A CHECK, never a verdict: the router normalises names, so a
                                raw diff reports drift that is not there. */}
                            {(probe.notOffered ?? []).length > 0 && (
                              <p className="prov-probe-note">
                                {t("models.probeNotOffered")}: {(probe.notOffered ?? []).join(", ")}
                              </p>
                            )}
                          </div>
                        ) : (
                          <span className="prov-probe-note">{probe.reason}</span>
                        ))}
                    </div>
                  )}

                  {isAdmin && (
                    <div className="prov-addmodel">
                      <input
                        className="form-input"
                        placeholder={t("models.namePh")}
                        aria-label={t("models.addToProvider", { prefix })}
                        value={isCustom ? newModel : draft.id}
                        onChange={(e) =>
                          isCustom
                            ? setNewModel(e.target.value)
                            : setDraft({ ...draft, id: e.target.value })
                        }
                      />
                      {!isCustom && (
                        <input
                          className="form-input"
                          placeholder={t("models.wirePh")}
                          aria-label={t("models.wirePh")}
                          value={draft.wire}
                          onChange={(e) => setDraft({ ...draft, wire: e.target.value })}
                        />
                      )}
                      {!isCustom && (
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm"
                          aria-expanded={advanced}
                          onClick={() => setAdvanced(!advanced)}
                        >
                          {t("models.advanced")}
                        </button>
                      )}
                      <button
                        type="button"
                        className="btn btn-primary btn-sm"
                        disabled={adding || (isCustom ? !newModel.trim() : !draft.id.trim())}
                        onClick={() => (isCustom ? void addModelToProvider(pv!) : void submitModel(prefix))}
                      >
                        {t("models.add")}
                      </button>
                    </div>
                  )}

                  {isAdmin && !isCustom && advanced && (
                    <div className="prov-facets">
                      <label>
                        <span>{t("models.nameLabel")}</span>
                        <input
                          className="form-input"
                          value={draft.name}
                          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                        />
                      </label>
                      <label>
                        <span>{t("models.ctxLabel")}</span>
                        <input
                          className="form-input"
                          inputMode="numeric"
                          value={draft.contextWindow}
                          onChange={(e) => setDraft({ ...draft, contextWindow: e.target.value })}
                        />
                      </label>
                      <label>
                        <span>{t("models.effortLabel")}</span>
                        <select
                          className="form-input"
                          value={draft.reasoningEffort}
                          onChange={(e) =>
                            setDraft({
                              ...draft,
                              reasoningEffort: e.target.value as typeof draft.reasoningEffort,
                            })
                          }
                        >
                          <option value="">{t("models.effortNone")}</option>
                          {["low", "medium", "high", "max"].map((lv) => (
                            <option key={lv} value={lv}>
                              {lv}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        <span>{t("models.maxLabel")}</span>
                        <input
                          className="form-input"
                          inputMode="numeric"
                          value={draft.maxTokens}
                          onChange={(e) => setDraft({ ...draft, maxTokens: e.target.value })}
                        />
                      </label>
                    </div>
                  )}

                  {isAdmin && mine.length > 0 && (
                    <div className="prov-disabled">
                      <span className="prov-disabled-label">{t("models.disabledTitle")}</span>
                      {mine.map((id) => (
                        <span key={id} className="prov-disabled-chip">
                          <code>{id}</code>
                          <button
                            type="button"
                            className="btn btn-ghost btn-mini"
                            disabled={busy !== null}
                            onClick={() => void enableModel(id)}
                          >
                            {t("models.enable")}
                          </button>
                        </span>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {/* The trailing ADD, as a dashed placeholder rather than a form that owns the
          top of the page: a provider is created once and configured rarely. */}
      {isAdmin && (
        <div className="prov-addrow">
          <button
            type="button"
            className="btn-dashed"
            onClick={() => setOpen(open === "@new" ? null : "@new")}
          >
            + {t("models.addCustomProvider")}
          </button>
          {open === "@new" && (
            <div className="prov-newform">
              <p className="prov-desc">{t("models.addCustomHint")}</p>
              <div className="prov-newgrid">
                <label>
                  <span>{t("models.prefix")}</span>
                  <input
                    className="form-input"
                    placeholder="my/"
                    value={newProvider.prefix}
                    onChange={(e) => setNewProvider({ ...newProvider, prefix: e.target.value })}
                  />
                </label>
                <label>
                  <span>{t("models.label")}</span>
                  <input
                    className="form-input"
                    value={newProvider.label}
                    onChange={(e) => setNewProvider({ ...newProvider, label: e.target.value })}
                  />
                </label>
                <label>
                  <span>{t("models.baseUrl")}</span>
                  <input
                    className="form-input"
                    placeholder="https://api.example.com/v1"
                    value={newProvider.baseURL}
                    onChange={(e) => setNewProvider({ ...newProvider, baseURL: e.target.value })}
                  />
                </label>
                <label>
                  <span>{t("models.apiKind")}</span>
                  <select
                    className="form-input"
                    value={newProvider.api}
                    onChange={(e) => setNewProvider({ ...newProvider, api: e.target.value })}
                  >
                    {apis.map((a) => (
                      <option key={a} value={a}>
                        {a}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  <span>
                    {t("models.apiKey")} <em className="muted">{t("models.keyKeep")}</em>
                  </span>
                  <input
                    className="form-input"
                    type="password"
                    value={newProvider.apiKey}
                    onChange={(e) => setNewProvider({ ...newProvider, apiKey: e.target.value })}
                  />
                </label>
              </div>
              <button
                type="button"
                className="btn btn-primary btn-sm"
                disabled={adding || !newProvider.prefix.trim() || !newProvider.baseURL.trim()}
                onClick={() => void submitProvider()}
              >
                {t("models.addCustomProvider")}
              </button>
            </div>
          )}
        </div>
      )}
    </>
  );
}
