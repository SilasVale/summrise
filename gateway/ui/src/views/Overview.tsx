import { useCallback, useEffect, useState } from "react";
import { CONSOLE_POLL_MS, deviceIsUp, deviceTally } from "../lib/deviceState.ts";
import { channelLabel, channelSignal, healthTone } from "../lib/channelState.ts";
import { Link } from "react-router-dom";
import { useAuth } from "../contexts/AuthContext.tsx";
import { useTranslation } from "../i18n.ts";
import { useToast } from "../contexts/ToastContext.tsx";
import {
  api,
  ApiError,
  type Device,
  type DeviceStatus,
  type HealthChannel,
  type ProviderView,
} from "../api/client.ts";
import { maskToken } from "../lib/format.ts";
import { Card, PageHeader, CopyButton } from "../components/ui.tsx";
// The tile reads the SAME list the Keys page renders. It kept its own eight-name copy until
// 2026-09-24, which is why the tile said "N/8" about an account the page called "N / 9".
import { KEY_NAMES } from "../lib/keyNames.ts";

// "DEEPSEEK_API_KEY" → "DEEPSEEK"; NVAPI_KEY has no "_API_KEY" suffix to strip.
const keyLabel = (name: string) => (name === "NVAPI_KEY" ? "NV" : name.replace("_API_KEY", ""));

function rel(ts?: number): string {
  if (!ts) return "—";
  const sec = Math.max(0, (Date.now() - ts) / 1000);
  if (sec < 90) return "<2m";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  return `${Math.floor(hr / 24)}d`;
}

/*
 * Overview v2 — the control deck. Asymmetric dashboard grid: a hero stat
 * band (four live numbers) + the gateway-token side card, the fleet strip,
 * then channel health beside the key matrix.
 *
 * Data semantics (user-reported bug fix): "online" here means the AGENT is
 * reachable (`agent_up`) — the same field the devices page counts. The old
 * overview read `online`, which is the browser-extension flag, so a healthy
 * device with no paired extension showed "offline".
 */
export default function Overview() {
  const { user, refreshUser } = useAuth();
  const { t } = useTranslation();
  const { toast } = useToast();
  const [tokenRevealed, setTokenRevealed] = useState(false);
  const [tokenNote, setTokenNote] = useState("");

  // `null` = not read yet OR the read FAILED — deliberately not `[]`, which the page
  // would render as "0/0 devices" and "no devices yet": a FALSE statement about a
  // fleet, made confidently, from a read that never succeeded.
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [devicesError, setDevicesError] = useState<string | null>(null);
  const [status, setStatus] = useState<Record<string, DeviceStatus>>({});
  const [channels, setChannels] = useState<HealthChannel[]>([]);
  const [users, setUsers] = useState<number | null>(null);
  // `null` for the same reason `devices` is: a read that has not happened (or FAILED) must not
  // be rendered as "you have none". The first-run hint below is the one thing on this page that
  // makes a claim about what is MISSING, so it is the one thing that must not guess.
  const [providers, setProviders] = useState<ProviderView[] | null>(null);

  const loadDashboard = useCallback(async () => {
    api
      .getHealth()
      .then((h) => setChannels(h.channels || []))
      .catch(() => {});
    // EVERY admin-only read lives behind ONE guard. The plugin probe used to sit
    // outside it, so a non-admin's landing page called an admin endpoint on load and
    // every 60 s — and a 401 from it signed them out (see the worker's mcp.ts).
    if (user?.role === "admin") {
      // THE DEVICE LIST IS THE STRAGGLER the paragraph above was written about and did not reach:
      // `GET /api/devices` calls `requireAdmin` in the worker, and this call used to sit OUTSIDE the
      // guard, so a non-admin's Overview hit an admin endpoint on load and every 60 seconds and
      // rendered the 403 as a red banner. It lives here now, which leaves `devices` and
      // `devicesError` at their initial `null` for a non-admin — the "not read" state this page
      // already distinguishes from "you have none" on purpose.
      api
        .getDevices()
        .then((d) => {
          setDevices(d.devices || []);
          setDevicesError(null);
        })
        .catch((err) => {
          setDevices(null);
          setDevicesError(err instanceof ApiError ? err.message : String(err));
        });
      api
        .getUsers()
        .then((u) => setUsers(u.users?.length ?? null))
        .catch(() => {});
      // Custom providers hold credentials TOO (`keyReady` is the record's own key resolving),
      // so "0/8 keys" is not "no credentials" — see the first-run hint.
      api
        .getProviders()
        .then((r) => setProviders(r.providers || []))
        .catch(() => setProviders(null));
      try {
        const s = await api.getPluginStatus(true);
        setStatus(s.devices || {});
      } catch {
        /* probe is best-effort — tiles keep their last value */
      }
    }
  }, [user?.role]);

  useEffect(() => {
    loadDashboard();
    // Same 60s cadence as the devices page — the two pages can never drift.
    const poll = setInterval(loadDashboard, CONSOLE_POLL_MS);
    return () => clearInterval(poll);
  }, [loadDashboard]);

  const handleRegenerate = async () => {
    const warn =
      user?.role === "admin"
        ? t("token.regenerateConfirm")
        : t("token.regenerateConfirm").split("\n")[0];
    if (!confirm(warn)) return;
    try {
      const data = await api.regenerateToken();
      if (data.token) {
        await refreshUser();
        setTokenRevealed(true);
        setTokenNote(t("token.regenerated"));
        toast(t("btn.regenerate"));
      }
    } catch (err) {
      toast(err instanceof ApiError ? err.message : t("token.regenerateFail"), true);
    }
  };

  const tokenDisplay = tokenRevealed ? user?.token || "" : maskToken(user?.token);

  const keyEntries = KEY_NAMES.map((name) => ({ name, info: user?.keys?.[name] }));
  const configuredCount = keyEntries.filter((k) => k.info?.configured).length;

  const isAdmin = user?.role === "admin";
  const { online: onlineCount } = deviceTally(devices, status);
  const channelsOk = channels.filter((c) => c.ok).length;

  /* ── the first-run hint ──────────────────────────────────────────────────────────────
   * A fresh operator lands on four zeroes and the page never said which action comes first.
   * Both lines are conditioned on data this page ALREADY reads, and both follow the rule the
   * tiles state for themselves: a read that failed is not a zero.
   *
   * "0/8 keys" is NOT "no credentials": a CUSTOM PROVIDER's record carries its own key, and
   * `keyReady` is that key resolving in this deployment. So the key line needs the provider
   * read to have SUCCEEDED and to name no ready key — anything less would tell an operator who
   * is already serving traffic to go and set up a key.
   */
  const noKeys = configuredCount === 0 && providers !== null && !providers.some((p) => p.keyReady);
  const noDevices = devices !== null && devices.length === 0 && isAdmin;
  const firstRun = noKeys || noDevices;

  const stats = [
    {
      label: t("stat.devices"),
      // "—" when we do not know, never "0/0" — and no link for a non-admin, who would
      // be redirected straight back to this page by `AdminOnly`.
      value: devices === null ? "—" : `${onlineCount}/${devices.length}`,
      tone: healthTone(devices !== null, onlineCount, devices?.length ?? 0),
      to: isAdmin ? "/devices" : undefined,
    },
    {
      label: t("stat.channels"),
      value: channels.length ? `${channelsOk}/${channels.length}` : "—",
      tone: healthTone(true, channelsOk, channels.length),
      // `/models` is where channel health lives (per-channel up/down/not-probed).
      // `/keys` contains no channel information at all — a tile about CHANNELS sent
      // you to a page about credentials.
      to: "/models",
    },
    {
      label: t("stat.keys"),
      value: `${configuredCount}/${keyEntries.length}`,
      tone: configuredCount > 0 ? "ok" : "off",
      to: "/keys",
    },
    {
      label: t("stat.users"),
      value: users === null ? "—" : String(users),
      tone: "info",
      to: users === null ? "/" : "/users",
    },
  ];

  return (
    <div>
      <PageHeader
        title={t("overview.title")}
        description={t("overview.lede")}
        actions={
          <button className="btn btn-secondary btn-sm" onClick={loadDashboard}>
            {t("btn.refresh")}
          </button>
        }
      />

      {/* ── first run: what to do next, said once and only when it is true ── */}
      {firstRun && (
        <Card title={t("overview.firstRun")}>
          {noKeys && (
            <p>
              {t("overview.firstRunKeys")}{" "}
              <Link to="/keys">
                {t("nav.keys")} →
              </Link>
            </p>
          )}
          {noDevices && (
            <p>
              {t("overview.firstRunDevices")}{" "}
              <Link to="/devices">
                {t("nav.devices")} →
              </Link>
            </p>
          )}
        </Card>
      )}

      {/* ── hero: stat band + token side card ── */}
      <div className="ov-hero">
        <div className="stat-band">
          {stats.map((s) => {
            // A card with nowhere to go is not a link. Rendering one for a non-admin
            // produced a click that `AdminOnly` silently redirected back here, which
            // reads as "nothing happened".
            const inner = (
              <>
                <span className="stat-value">{s.value}</span>
                <span className="stat-label">{s.label}</span>
              </>
            );
            const cls = `stat-card stat-${s.tone}`;
            return s.to ? (
              <Link key={s.label} to={s.to} className={cls}>
                {inner}
              </Link>
            ) : (
              <div key={s.label} className={cls}>
                {inner}
              </div>
            );
          })}
        </div>

        <Card
          className="ov-token"
          title={t("token.title")}
          description={<span dangerouslySetInnerHTML={{ __html: t("token.desc") }} />}
        >
          <div className="token-row">
            <code className="token">{tokenDisplay}</code>
            <CopyButton
              text={user?.token || ""}
              tone="primary"
              onCopied={() => toast(t("token.copied"))}
            />
            <button className="btn btn-ghost" onClick={() => setTokenRevealed(!tokenRevealed)}>
              {tokenRevealed ? t("btn.hide") : t("btn.show")}
            </button>
          </div>
          <div className="token-actions">
            <button className="btn btn-danger btn-sm" onClick={handleRegenerate}>
              {t("btn.regenerate")}
            </button>
          </div>
          {tokenNote && <p className="form-message form-message-success">{tokenNote}</p>}
        </Card>
      </div>

      {/* ── fleet strip ── */}
      <Card
        title={t("overview.devicesTitle")}
        headerExtra={
          isAdmin ? (
            <Link className="card-link" to="/devices">
              {t("overview.viewAll")} →
            </Link>
          ) : undefined
        }
      >
        {devicesError ? (
          <div className="banner-error">
            <span>
              {t("overview.devicesFail")} — {devicesError}
            </span>
            <button className="btn btn-ghost btn-mini" onClick={() => void loadDashboard()}>
              {t("devices.retry")}
            </button>
          </div>
        ) : devices === null ? (
          <p className="muted">{t("loading")}</p>
        ) : devices.length === 0 ? (
          <p className="muted">{t("overview.devicesEmpty")}</p>
        ) : (
          <div className="dev-strip">
            {devices.map((d) => {
              const st = status[d.name] || {};
              const up = deviceIsUp(st);
              // NO CONDITIONAL CLASS ON THIS LINK (round 78). It carried `dev-mini online`, and no rule in either
              // sheet has ever matched `online` — the design sweep reported it as "on screen, matched by nothing" on
              // the Overview. The state it named is already carried by the LED beside it (`dev-mini-led on`, a filled
              // mark against the base ring), so the class was a SECOND source for one fact with no paint of its own:
              // the objective asks for one source per fact, and for whatever stops earning its place to be pruned.
              // The LED is the source. (A JSX comment here is a syntax error: this is the top of a return.)
              return (
                <Link key={d.name} to="/devices" className="dev-mini">
                  <span className={`dev-mini-led${up ? " on" : ""}`} />
                  <span className="dev-mini-name">{d.name}</span>
                  <span className="dev-mini-meta">
                    {up
                      ? `v${st.version || d.lastVersion || "?"} · ${rel(st.checked_at)}`
                      : t("overview.offline")}
                  </span>
                </Link>
              );
            })}
          </div>
        )}
      </Card>

      {/* ── health + keys ── */}
      <div className="ov-cols">
        <Card title={t("overview.healthTitle")}>
          {channels.length === 0 ? (
            <p className="muted">—</p>
          ) : (
            <div className="health-list">
              {channels.map((c) => (
                <div key={c.id} className="health-row">
                  <span className={`dot ${channelSignal(c.ok)}`} />
                  <span className="health-id">{c.id}</span>
                  <span className="health-model">{c.model}</span>
                  <span className={`health-state ${channelSignal(c.ok)}`}>
                    {channelLabel(c, t)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </Card>

        <Card
          title={t("keys.title")}
          description={t("overview.keysHint")}
          headerExtra={
            <Link className="card-link" to="/keys">
              {t("nav.keys")} →
            </Link>
          }
        >
          <div className="ov-keylist">
            {keyEntries.map(({ name, info }) => (
              <div key={name} className="ov-keyrow">
                <span className={`ov-keyled${info?.configured ? " on" : ""}`} />
                <span className="ov-keyname">{keyLabel(name)}</span>
                <span className="ov-keystate">
                  {info?.configured ? t("key.configured") : t("key.notConfigured")}
                </span>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  );
}
