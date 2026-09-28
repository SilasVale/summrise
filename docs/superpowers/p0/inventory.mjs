// P0's inventory, as DATA. Every count in the document is printed by this file, so the number and
// the list cannot drift apart: change a classification here and the count changes with it.
//
//   node inventory.mjs <repo-root>            -> the two tables and the counts
//
// THE RULE (the plan's own): an export is LOGIC if it computes, parses, derives, validates, formats
// or decides; RENDERING if it only places what it was given; BOUNDARY if it only talks to a browser
// or network API. A file can hold all three — this is classified per EXPORT, which is the point.
import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";

const LOGIC = {
  "agent/resources/panel-react/src": {
    "components/ApprovalGate.tsx": ["firstWord"],
    "components/CommandCard.tsx": ["fmtDuration"],
    "components/IconRail.tsx": ["PAGE_ICONS"],
    "components/RunStrip.tsx": ["clock"],
    "components/Shell.tsx": ["Page", "PAGES", "PAGE_LABELS"],
    "components/TabBar.tsx": ["SessionView"],
    "components/TerminalWorkspace.tsx": ["CommandEvents"],
    "components/UpdateCard.tsx": ["UpdateStatus", "parseUpdateStatus", "parseAttempt", "attemptAge", "checkedAge"],
    "ui/Icon.tsx": ["IconName"],
    "hooks/useAgentVitals.ts": ["AgentVitals", "BootKind", "LastBoot", "parseLastBoot", "fmtUptime"],
    "hooks/useAiActivityPulse.ts": ["PULSE_MS"],
    "hooks/useAttention.ts": ["humanMs"],
    "hooks/useBootHistory.ts": ["BootRecord", "BootHistory", "EMPTY_BOOT_HISTORY", "parseBootHistory"],
    "hooks/useCommandEvents.ts": ["CommandEvent", "CommandCard", "terminalStatus", "groupEvents"],
    "hooks/useDeviceActivity.ts": ["WORKING_MS"],
    "hooks/useDeviceRead.ts": ["DeviceReadOptions", "DeviceRead"],
    "hooks/useMonitors.ts": ["MonitorTarget", "Monitors", "EMPTY_MONITORS", "parseMonitors", "MonitorAlert", "parseMonitorChange", "fmtSince", "unstableTargets", "downTargets"],
    "hooks/useSessions.ts": ["PendingApproval", "mapPending", "pendingApprovalCount", "Session"],
    "hooks/useTrajectory.ts": ["TrajRound", "groupRounds"],
    "hooks/useVitalsSeries.ts": ["VitalsSeries", "EMPTY_SERIES", "parseVitalsSeries"],
    "lib/agentVersion.ts": ["releaseVersion", "releaseVersionLabel"],
    "lib/ansi.ts": ["stripAnsi"],
    "lib/api.ts": ["deviceRefused"],
    "lib/archive.ts": ["ArchiveEntry", "ARCHIVE_PAGE", "archiveEntries", "newestFirst", "pageOf", "archiveClock", "lastEventWords"],
    "lib/attention.ts": ["stateKey", "shouldNotify", "AttentionItem", "BASE_TITLE", "attentionFrom", "titleFor", "badgeIcon", "attentionSummary"],
    "lib/bootNotice.ts": ["bootKindLabel", "isCrash", "REPLACED_NOTICE_SECS", "bootNotice"],
    "lib/browserAction.ts": ["actionVerdict"],
    "lib/contract.gen.ts": ["FRAMES", "BOOT_KINDS", "END_REASONS", "EXITED_PREFIX", "Frame", "BootKind", "EndReason"],
    "lib/duration.ts": ["humanIdle"],
    "lib/evicted.ts": ["EvictionNotice", "parseEvicted", "evictedText", "humanIdle"],
    "lib/gettingStarted.ts": ["GETTING_STARTED_VERSION", "GETTING_STARTED_KEY", "STEPS", "GETTING_STARTED_LEAD", "GETTING_STARTED_REOPEN", "shouldShowGuide"],
    "lib/idleSessions.ts": ["IDLE_OFFER_MS", "idleSessions", "idleOfferText", "humanIdle"],
    "lib/lagMarkers.ts": ["pruneLagMarkers"],
    "lib/liveness.ts": ["Liveness", "URGENCY", "SILHOUETTE", "MOVES", "livenessOf", "deviceLiveness", "sessionWaiting", "sessionActive", "anyCommandRunning", "sessionLiveness", "sessionFailed"],
    "lib/monitorMark.ts": ["monitorModifier", "monitorMarkClass"],
    "lib/notify.ts": ["NotifyPermission", "NotificationLike", "NotificationCtor", "readPermission", "permissionHint"],
    "lib/path.ts": ["PATH_STATES", "PathState", "stateFromEnd", "cardState", "PathStep", "PathSummary", "SessionPath", "derivePath", "summarizePath", "attentionSteps"],
    "lib/readState.ts": ["ReadState"],
    "lib/recipe.ts": ["RECIPE_MARKER", "RECIPE_TAG", "suggestedTitle", "buildRecipe", "recipeWarnings"],
    "lib/runs.ts": ["OperationEvent", "RunBoundary", "ActivityRow", "RunGroup", "groupOperation", "operationRows", "groupCount", "RUN_STATE_LABEL", "runStateNote"],
    "lib/sessionLabels.ts": ["disambiguateLabels"],
    "lib/sessionViews.ts": ["pruneSessionViews"],
    "lib/spark.ts": ["LOAD_MIN_SAMPLES", "LOAD_WINDOW_MS", "sparkSegments", "seriesStats", "loadNotice"],
    "lib/terminalAdopt.ts": ["MAX_ADOPT_PAGES", "adoptNeedsAnotherPage", "adoptPageExceeded", "WRITE_SLICE_CHARS", "splitWriteSlices"],
    "lib/trailRead.ts": ["trailReadNotice"],
    "lib/updateDiagnosis.ts": ["UpdateDiagnosis", "diagnoseUpdate"],
  },
  "gateway/ui/src": {
    "api/client.ts": ["ApiError", "ProviderView", "ModelFacets", "ProbeResult", "ProviderModelDraft", "Me", "RouteInfo", "HealthChannel", "User", "Device", "DeviceStatus", "RegKeyInfo"],
    "i18n.ts": ["TranslationKey", "t"],
    "lib/channelState.ts": ["channelSignal", "channelLabel", "healthTone"],
    "lib/deviceState.ts": ["tunnelKnownDown", "deviceIsUp", "agentSignal", "tunnelSignal", "CONSOLE_POLL_MS", "deviceTally", "DeviceTally", "Signal"],
    "lib/deviceUpdate.ts": ["rememberedUpdateAttempt", "updateControl"],
    "lib/format.ts": ["maskToken"],
    "lib/keyNames.ts": ["KEY_NAMES"],
    "lib/lane.ts": ["barePrefix", "laneClass"],
  },
};

function exportsOf(dir) {
  const files = execSync(`find ${dir} -name '*.ts' -o -name '*.tsx'`, { encoding: "utf8" }).trim().split("\n")
    .filter((f) => !/\.test\.|__tests__|test-setup|test-utils|\.d\.ts$/.test(f)).sort();
  const out = {};
  for (const f of files) {
    const src = readFileSync(f, "utf8");
    const names = new Set();
    for (const m of src.matchAll(/^export\s+(?:async\s+)?(?:function|const|let|var|class|type|interface|enum)\s+([A-Za-z0-9_$]+)/gm)) names.add(m[1]);
    for (const m of src.matchAll(/^export\s+(?:type\s+)?\{([^}]*)\}/gm))
      for (const p of m[1].split(",")) { const n = p.trim().split(/\s+as\s+/).pop().trim(); if (n && n !== "default") names.add(n); }
    if (names.size) out[f.replace(dir + "/", "")] = [...names];
  }
  return out;
}

const ROOT = process.argv[2] || "/tmp/rustmig";
let grand = 0, grandFiles = 0;
for (const [rel, map] of Object.entries(LOGIC)) {
  const ex = exportsOf(`${ROOT}/${rel}`);
  let logic = 0, files = 0, rendered = 0, boundary = 0;
  const rows = [];
  for (const [file, all] of Object.entries(ex)) {
    const L = map[file] || [];
    const missing = L.filter((n) => !all.includes(n));
    if (missing.length) throw new Error(`${rel}/${file}: classified but not exported: ${missing.join(", ")}`);
    const other = all.filter((n) => !L.includes(n));
    if (L.length) { logic += L.length; files++; }
    rendered += other.length;
    rows.push({ file, L, other, all });
  }
  console.log(`\n### ${rel === "agent/resources/panel-react/src" ? "The panel — " : "The console — "}${rel}\n`);
  console.log("| file | LOGIC (migration candidates) | RENDERING / BOUNDARY (stays TS) |");
  console.log("|---|---|---|");
  for (const r of rows.sort((a, b) => a.file.localeCompare(b.file)))
    console.log(`| \`${r.file}\` | ${r.L.length ? r.L.map((n) => "`" + n + "`").join(" · ") : "—"} | ${r.other.length ? r.other.map((n) => "`" + n + "`").join(" · ") : "—"} |`);
  console.log(`\n**${logic} LOGIC exports across ${files} files** (of ${Object.keys(ex).length} files exporting anything; the other ${rendered} exports are rendering or boundary).`);
  grand += logic; grandFiles += files;
}
console.log(`\n## TOTAL\n\n**${grand} logic exports across ${grandFiles} files** (panel + console).`);
