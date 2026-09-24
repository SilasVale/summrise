import { useState, useEffect, useCallback } from "react";
import { useAuth } from "../contexts/AuthContext.tsx";
import { Link } from "react-router-dom";
import { useTranslation } from "../i18n.ts";
import { useToast } from "../contexts/ToastContext.tsx";
import { api } from "../api/client.ts";
import { clientBase } from "../lib/baseUrl.ts";
import { Card, PageHeader, Badge, CopyButton } from "../components/ui.tsx";

export default function RoutesView() {
  const { user } = useAuth();
  const { t } = useTranslation();
  const { toast } = useToast();
  const [current, setCurrent] = useState<string | null>(null);
  const [apiHost, setApiHost] = useState("");
  const [loadFailed, setLoadFailed] = useState(false);
  const [loading, setLoading] = useState(true);
  const [usproxyOn, setUsproxyOn] = useState(false);
  const [usproxyLoading, setUsproxyLoading] = useState(false);

  const loadChannels = useCallback(async () => {
    try {
      const [route, publicInfo, proxy] = await Promise.all([
        api.getRoute().catch(() => null),
        api.getPublicRoutes().catch(() => null),
        api.getUsProxy().catch(() => null),
      ]);

      if (route?.effective) setCurrent(route.effective);
      else if (route?.model) setCurrent(route.model);
      if (publicInfo?.apiHost) setApiHost(publicInfo.apiHost);
      if (proxy?.enabled !== undefined) setUsproxyOn(proxy.enabled);
      // THE CATCH BELOW CANNOT FIRE, and that is why nothing was ever said: each read carries its own
      // `.catch(() => null)` (deliberately — one failing read must not blank the others), so
      // `Promise.all` cannot reject. Three nulls therefore means NOTHING WAS READ, which used to
      // render as the DEFAULT host — a plausible, copy-ready client config claiming to be this
      // gateway's, inside the JSON block below. A failure indistinguishable from success is stated
      // instead: the toast keeps its string, and the flag keeps it on screen.
      if (!route && !publicInfo && !proxy) {
        setLoadFailed(true);
        toast(t("route.loadFail"), true);
      } else setLoadFailed(false);
    } catch {
      // Unreachable today; kept because a future edit that drops one of the per-read catches must
      // still land somewhere that tells the operator.
      setLoadFailed(true);
      toast(t("route.loadFail"), true);
    }
    setLoading(false);
  }, [toast, t]);

  useEffect(() => {
    loadChannels();
  }, [loadChannels]);

  const handleClearRoute = async () => {
    try {
      await api.setRoute(null);
      toast(t("route.switched"));
      await loadChannels();
    } catch {
      toast(t("route.fail"), true);
    }
  };

  const handleToggleProxy = async () => {
    setUsproxyLoading(true);
    try {
      await api.setUsProxy(!usproxyOn);
      toast(t("usproxy.switched"));
      setUsproxyOn(!usproxyOn);
    } catch {
      toast(t("usproxy.fail"), true);
    }
    setUsproxyLoading(false);
  };

  // Client example
  const modelKeys = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
  ];
  const base = clientBase(apiHost);
  const token = user?.token || "<your gateway token>";
  const envConfig: Record<string, string> = { ANTHROPIC_BASE_URL: base, ANTHROPIC_API_KEY: token };
  for (const k of modelKeys) envConfig[k] = "auto[1m]";
  const clientExample = JSON.stringify({ env: envConfig }, null, 2);

  if (loading) {
    return (
      <div>
        <PageHeader title={t("nav.routes")} />
        <Card>
          <p className="muted">{t("loading")}</p>
        </Card>
      </div>
    );
  }

  return (
    <div>
      <PageHeader title={t("nav.routes")} description={<span dangerouslySetInnerHTML={{ __html: t("routes.lede") }} />} />

      {/* A PERSISTENT MARKER, not only the toast: the toast scrolls away, and what it warns about —
          the copy-ready config below — stays on screen. The markup is the loading card's, so this
          adds no visual vocabulary for the design gates to judge. */}
      {loadFailed && (
        <Card>
          <p className="muted">{t("route.loadFail")}</p>
        </Card>
      )}

      {/* US Proxy toggle */}
      {user?.role === "admin" && (
        <Card
          title={t("usproxy.title")}
          description={t("usproxy.desc")}
          headerExtra={
            <Badge tone={usproxyOn ? "success" : "warning"}>
              {usproxyOn ? t("usproxy.on") : t("usproxy.off")}
            </Badge>
          }
        >
          {/* THE BUTTON NAMES ITS ACTION. It used to read "Toggle", which says what
              KIND of control it is and nothing about what pressing it will do — while
              the badge beside it already carries the current state. A control whose
              label is a category forces the reader to work out the polarity from the
              state pill and then invert it; naming the outcome removes the step. */}
          <button className="btn btn-secondary btn-sm" disabled={usproxyLoading} onClick={handleToggleProxy}>
            {usproxyOn ? t("usproxy.turnOff") : t("usproxy.turnOn")}
          </button>
        </Card>
      )}

      {/* CURRENT ROUTE — a summary, not a second switcher.
          This card used to list one row per channel with a "use" button, which
          DUPLICATED the Models page and did it worse: it showed only each
          channel's HEALTH-PROBE model, so a channel offering eight models
          appeared to offer one. Choosing belongs where the catalogue is; this
          says what is chosen and links there. */}
      <Card
        title={t("route.title")}
        description={<span dangerouslySetInnerHTML={{ __html: t("route.desc") }} />}
        headerExtra={
          <button className="btn btn-ghost btn-sm" onClick={handleClearRoute}>
            {t("route.auto")}
          </button>
        }
      >
        <div className="current-route">
          <code>{current || t("route.none")}</code>
          <Link className="btn btn-secondary btn-sm" to="/models">
            {t("route.pickInModels")}
          </Link>
        </div>
      </Card>

      {/* Client example */}
      <Card title={t("client.title")}>
        <pre style={{ marginTop: 12 }}>
          <code>{clientExample}</code>
        </pre>
        <div className="row mt-12">
          <CopyButton text={clientExample} small onCopied={() => toast(t("token.copied"))} />
        </div>
        <p className="muted mt-12" dangerouslySetInnerHTML={{ __html: t("client.note") }} />
      </Card>
    </div>
  );
}
