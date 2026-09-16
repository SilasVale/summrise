// PanelApp — the panel-density app: Shell (panel density) with the context
// rail (sessions/plugins) + status bar. Renders the SAME pages as the desktop
// density (design doc §4). The old AppFrame three-column wiring + Sidebar
// dual-view are replaced by Shell + ContextRail.
import { useState } from "react";
import { pendingApprovalCount, type Session } from "../hooks/useSessions";
import { EvictedNotice } from "./EvictedNotice";
import { IdleSessionsBar } from "./IdleSessionsBar";
import { GettingStarted } from "./GettingStarted";
import { HistoryPage } from "./HistoryPage";
import { IconRail } from "./IconRail";
import { Shell, type Page } from "./Shell";
import { idleSessions } from "../lib/idleSessions";
import { useEvictedNotice } from "../hooks/useEvicted";
import { GETTING_STARTED_KEY, GETTING_STARTED_VERSION, shouldShowGuide } from "../lib/gettingStarted";
import { ContextRail } from "./ContextRail";
import { StatusBar } from "./StatusBar";
import { useAgentVitals } from "../hooks/useAgentVitals";
import { useBootHistory } from "../hooks/useBootHistory";
import { useVitalsSeries } from "../hooks/useVitalsSeries";
import { useMonitorAlerts, useMonitors } from "../hooks/useMonitors";
import {
  useAttention,
  useAttentionNotifications,
  useAttentionTitle,
  useNotifyPermission,
} from "../hooks/useAttention";
import { MonitorAlerts } from "./MonitorAlerts";
import { TerminalWorkspace, type CommandEvents } from "./TerminalWorkspace";
import { BrowserPage } from "./BrowserPage";
import { MemoryPage } from "./MemoryPage";
import { PluginsPage } from "./PluginsPage";
import { SettingsPage } from "./SettingsPage";
import { ConnModal } from "./ConnModal";
import type { SessionView } from "./TabBar";
import type { usePlugins } from "../hooks/usePlugins";

interface Props {
  /** The host this panel was reached on — the device's identity in the chrome. */
  host?: string;
  sessions: Session[];
  activeSid: string | null;
  onActivate: (sid: string) => void;
  onClose: (sid: string) => void;
  onExport: (sid: string) => void;
  onViewChange: (sid: string, v: SessionView) => void;
  /** App owns the view map; this shell only renders from it (see TerminalWorkspace). */
  sessionViews: Record<string, SessionView>;
  /** Hand the session's keyboard to a person / back to the AI. */
  onSetControl: (sid: string, human: boolean) => Promise<unknown>;
  /** Arm/disarm the approval gate for a session. */
  onSetApproval: (sid: string, required: boolean) => Promise<unknown>;
  /** Answer a pending approval request (`grant` also remembers it). */
  onDecideApproval: (
    sid: string,
    id: string,
    approve: boolean,
    grant?: boolean,
  ) => Promise<unknown>;
  /** Revoke one approval grant, or every one when omitted. */
  onRevokeGrants: (sid: string, grant?: string) => Promise<unknown>;
  /** State the session's goal, or clear it with an empty string. */
  onSetGoal: (sid: string, goal: string) => Promise<unknown>;
  registerWrite: (
    sid: string,
    fn: (bytes: Uint8Array) => void,
    getRendered: () => number,
  ) => (() => void) & { unregister?: (sid: string) => void };
  onNewSession: (kind: "pty" | "ssh" | "serial" | "browser") => void;
  status: string;
  sseState: string;
  token: string;
  plugins: ReturnType<typeof usePlugins>;
  cmdEvents: CommandEvents;
  connModal: "ssh" | "serial" | null;
  onConnClose: () => void;
  onConnConnect: (
    kind: "ssh" | "serial",
    target: string,
    extra: Record<string, unknown>,
  ) => Promise<unknown>;
}

export function PanelApp(props: Props) {
  // Device vitals for the instrument line: ONE owner for the poll (see the hook).
  const vitals = useAgentVitals();
  // The restart history, polled ONCE here and handed to both consumers: the strip's crash
  // chip (which says how many there have been) and the Settings card (which lists them).
  const restarts = useBootHistory();
  // The vitals SERIES, polled once here (see useVitalsSeries): the strip's sustained-load
  // chip and the Settings card's charts read the same array.
  const vitalsSeries = useVitalsSeries();
  // The reachability monitors, polled once per shell: the strip's down chip and the Settings
  // card read the same list, and the card's actions are the hook's own.
  const monitors = useMonitors();
  // The device SPEAKS about a watched target changing state — the one monitor fact that is only
  // useful now (see MonitorAlerts).
  const monitorAlerts = useMonitorAlerts();
  // ── GETTING YOUR ATTENTION (round 264): the title, the badge, and — if the operator allows it —
  //    a desktop notification. The first two need no permission and always work; the third is the
  //    one channel that leaves the browser.
  // The WAITING question, not the armed gate: `pendingApprovalCount` is the panel's one rule for
  // "an AI is blocked on a human", and it is what the strip's waiting chip already reads.
  const attention = useAttention(monitors, pendingApprovalCount(props.sessions));
  useAttentionTitle(attention);
  const [notifyPermission, requestNotifyPermission] = useNotifyPermission();
  useAttentionNotifications(attention, notifyPermission, notifyPermission === "granted");
  /** The card's "Send a test": the user gesture the browser requires, then one notification that
   *  says what the channel is for. Sent even with nothing wrong, because the point is to find out
   *  whether the OS will actually show it (Do Not Disturb is invisible from here). */
  const testNotification = async () => {
    const state = notifyPermission === "granted" ? notifyPermission : await requestNotifyPermission();
    if (state !== "granted" || typeof Notification === "undefined") return;
    try {
      new Notification("Vale", {
        body: "This is how a watched host going down will reach you.",
        tag: "vale-test",
      });
    } catch {
      /* the browser refused at the last moment — the card shows the permission state it reported */
    }
  };
  const [page, setPage] = useState<Page>("terminal");
  const connected = props.sseState === "connected";
  // THE FIRST FIVE MINUTES, ONCE. Read through a try/catch because private-mode storage throws on
  // access (the same rule `boot.ts` learned): a browser that refuses to remember must still get the
  // guide, not a blank panel.
  const [guideOpen, setGuideOpen] = useState(() => {
    try {
      return shouldShowGuide(localStorage.getItem(GETTING_STARTED_KEY));
    } catch {
      return true;
    }
  });
  // The device announces evictions on the same stream the monitors use; this is the one line
  // that says what it took (see hooks/useEvicted).
  const evicted = useEvictedNotice();
  const closeGuide = () => {
    setGuideOpen(false);
    try {
      localStorage.setItem(GETTING_STARTED_KEY, GETTING_STARTED_VERSION);
    } catch {
      /* a browser that cannot remember will simply show it again */
    }
  };

  return (
    <>
      {/* The device's own announcement, above the shell: a session it took away (cap or idle TTL),
          with the rule that took it. Nothing renders when nothing happened. */}
      <EvictedNotice notice={evicted} />
      {/* The offer to shed what nobody is using — nothing drawn when there is nothing to offer
          (see lib/idleSessions for the threshold and why it is an hour). It closes through the same
          callback the tabs' own × uses, so there is one close path in this density. */}
      <IdleSessionsBar candidates={idleSessions(props.sessions)} onClose={props.onClose} />
      <Shell
        density="panel"
        iconRail={
          <IconRail
            page={page}
            onPageChange={setPage}
            connected={connected}
            pendingCount={pendingApprovalCount(props.sessions)}
            onOpenGuide={() => setGuideOpen(true)}
          />
        }
        contextRail={
          page === "terminal" || page === "plugins" ? (
            <ContextRail
              page={page}
              sessions={props.sessions}
              activeSid={props.activeSid}
              onActivate={props.onActivate}
              onNewSession={props.onNewSession}
              plugins={props.plugins}
              connected={connected}
            />
          ) : undefined
        }
        statusBar={
          <StatusBar
            recentCrashes={restarts.summary.crashes}
            vitalsSeries={vitalsSeries}
            monitors={monitors}
            sessions={props.sessions}
            status={props.status}
            sseState={props.sseState as "connected" | "down" | "connecting"}
            vitals={vitals}
            identity={props.host}
          />
        }
        canvas={
          <div id="panel-main">
            {props.connModal && (
              <ConnModal
                kind={props.connModal}
                onClose={props.onConnClose}
                onConnect={(target, extra) =>
                  props.onConnConnect(props.connModal!, target, extra)
                }
              />
            )}
            {page === "terminal" && (
              /* The terminal page's name lives in the outline only: every visible row belongs to
                 the terminal, and a title would cost one. */
              <h1 className="sr-only">Terminal</h1>
            )}
            {page === "terminal" && (
              <TerminalWorkspace
                sessions={props.sessions as any}
                activeSid={props.activeSid}
                onActivate={props.onActivate}
                onClose={props.onClose}
                onExport={props.onExport}
                onViewChange={props.onViewChange}
                sessionViews={props.sessionViews}
                onSetControl={props.onSetControl}
                onSetApproval={props.onSetApproval}
                onDecideApproval={props.onDecideApproval}
                onRevokeGrants={props.onRevokeGrants}
                onSetGoal={props.onSetGoal}
                registerWrite={props.registerWrite}
                cmdEvents={props.cmdEvents}
                token={props.token}
                density="panel"
                sseState={props.sseState as "connected" | "down" | "connecting"}
              />
            )}
            {/* The device speaking about a watched target (see MonitorAlerts): above the page, never
                over it, and gone on its own. */}
            <MonitorAlerts alerts={monitorAlerts} />
            {page === "history" && <HistoryPage sessions={props.sessions} />}
            {page === "browser" && <BrowserPage token={props.token} />}
            {page === "memory" && <MemoryPage />}
            {page === "plugins" && <PluginsPage plugins={props.plugins} />}
            {page === "settings" && (
              <SettingsPage
                onOpenMemory={() => setPage("memory")}
                restarts={restarts}
                restartsFailed={restarts.failed}
                vitals={vitalsSeries}
                vitalsFailed={vitalsSeries.failed}
                monitors={monitors}
                monitorsFailed={monitors.failed}
                onMonitorAdd={monitors.add}
                onMonitorRemove={monitors.remove}
                onMonitorProbe={monitors.probe}
                notifyPermission={notifyPermission}
                onRequestNotify={requestNotifyPermission}
                onTestNotify={testNotification}
                attention={attention}
                runningRelease={vitals.release}
              />
            )}
          </div>
        }
      />
      {guideOpen && (
        <GettingStarted
          onClose={closeGuide}
          onGoTo={(p) => {
            setPage(p);
            closeGuide();
          }}
        />
      )}
    </>
  );
}
