// The console's HTTP surface, typed. Every call carries the session token once there is one; the
// Cloudflare Access path needs no token, and /session says which of the two answered.

export type Session = {
  authenticated: boolean;
  email?: string;
  via?: string;
  /// What the console made of the Cloudflare Access identity on this request: whether the trust is
  /// configured, whether the header arrived, and why it was refused. The page draws a login box when
  /// none of it is an identity, and these three facts are the sentence it can show instead of one.
  access?: { configured: boolean; header: boolean; error?: string };
};
/**
 * One baseline generation the launcher wrote: the version, and the channel it came from.
 *
 * The device reads it from `<install>/baseline/<generation>/baseline.json` for display — what the
 * launcher wrote, not a signature check the console makes. A missing or half-written sidecar arrives
 * as `null`, never as a partial object.
 */
export type UpdateGeneration = { version?: string; channel?: string };

/**
 * The launcher's last swap, from `<install>/swap.result.json`.
 *
 * `applied` is a pair that took over, `rolled_back` a pair that failed and returned to `previous`,
 * and `refused` a swap the launcher declined before it touched anything — a refusal is not an
 * update. The device reports `null` for a missing, torn or unrecognised result, and leaves the key
 * out on a device too old to report it.
 */
export type UpdateSwap = {
  outcome: "applied" | "rolled_back" | "refused";
  version?: string;
  at_ms: number;
  reason?: string;
};

/**
 * One restore the launcher recorded in `<install>/repair.log`: the app copy no longer matched the
 * generation it came from, so the launcher rewrote it. `from` is the generation the bytes came from;
 * `sha256` is the launcher's hash of them, kept out of the visible text.
 */
export type UpdateRepair = { at_ms: number; from: "baseline" | "previous"; sha256: string };

/**
 * The device's own update story, as its host frame carries it: the version it actually runs, the
 * verdict of its last start, the updater's last answer, and when it last checked. Every field is
 * the device's; the console does not fill in a missing one, because "unknown" and "none" are
 * different facts.
 *
 * It is a separate section of the frame from [`HostFacts.update`], which is the launcher's files on
 * disk. This one comes from the state document the running app writes.
 */
export type DeviceUpdate = {
  /// The version the process actually runs, from the state document's `install.version`.
  current?: string;
  /// The device's verdict on its last start (or on the updater's last answer): the outcome, and the
  /// sentence the device wrote. `detail` is what the page shows; the console never composes one.
  verdict?: { outcome: string; detail: string } | null;
  /// The updater's last answer, present only after a swap result was folded in. A staged release is
  /// not a result, so this is absent until one exists.
  last_update?: { to?: string; outcome: string; at_ms: number } | null;
  /// When the device last looked for a release, and what asked it to (`start`, `channel`,
  /// `schedule`). The device's own state records this beside its update phase; a device whose state
  /// predates the field omits it, so "not recorded" is not drawn as "never checked".
  last_check?: { at_ms: number; trigger?: string } | null;
  /// The version a rollback returned to, present only when the last answer was `rolled_back`.
  rolled_back_to?: string;
};

/** What the device sent about its machine on the channel: the console shows it without a grant. */
export type HostFacts = {
  os?: { name?: string; release?: string; version?: string; arch?: string };
  machine?: string;
  uptime_s?: number;
  cpu?: { cores?: number };
  memory?: { total_bytes?: number; available_bytes?: number };
  disks?: { name: string; total_bytes: number; free_bytes: number }[];
  sessions?: { id: number; user?: string | null; state: string }[];
  helper?: { registered?: boolean; pid?: number | null; session?: number | null };
  /**
   * The five-minute `SummriseRepair` task, read from the machine's own Task Scheduler: whether the
   * task is registered, its last run and result, and when it runs next. `registered: false` is the
   * task being absent; the whole key is absent on a device too old to report it or when the
   * machine could not answer. The Machine tab draws the absent key for an online machine as
   * `not reported by this build` (naming the running version) rather than reading its silence as a
   * health, and draws no row for an offline machine's dated facts.
   */
  repair_task?: {
    registered: boolean;
    last_run_ms?: number;
    last_result?: number;
    next_run_ms?: number;
  };
  /**
   * This app's own boot journal, newest first and bounded by the device (ten). The newest entry is
   * the moment the running process started, which is what an updater's answer must follow to belong
   * to this process. Absent on a device that predates the journal, so a page cannot invent a start.
   */
  restarts?: {
    at_ms: number;
    version?: string;
    sha256?: string;
    channel?: string;
    pid?: number;
  }[];
  /// The launcher's three baseline generations: `current` is the pair it trusts, `pending` is a pair
  /// staged and not yet proven, `previous` is the pair a failed update returns to. Each generation is
  /// `null` when its sidecar is missing or unreadable, so a device with no baselines reports three
  /// nulls; the whole key is absent on a device too old to report it.
  update?: {
    current?: UpdateGeneration | null;
    previous?: UpdateGeneration | null;
    pending?: UpdateGeneration | null;
    /// The launcher's last swap result. `null` when the file is missing, torn or names an outcome
    /// this console does not know — never an inferred restart.
    last_swap?: UpdateSwap | null;
    /// The launcher's newest restores, newest first and at most five; empty when it never repaired
    /// anything (or the log is missing), and absent on a device too old to report it.
    repairs?: UpdateRepair[];
  } | null;
};

/** A saved terminal preset: the shape of a connection, never its credential. */
export type SavedConnection = {
  id: string;
  name: string;
  kind: string;
  host?: string;
  port?: number;
  user?: string;
  port_path?: string;
  baud?: number;
};

/**
 * One tool as the device's live `tools/list` advertises it, plus the console's two computed fields.
 * `source` is `"builtin"` or `"plugin:<namespace>"`; `capability` is the built-in group or the
 * plugin's namespace. This is the page's source of truth: the static registry is a fallback only.
 */
export type LiveTool = {
  name: string;
  description: string;
  inputSchema: unknown;
  source: string;
  capability: string;
  destructive: boolean;
};

/**
 * One tool as the console's static registry serves it at `/tools`: the built-ins a fleet token can
 * grant, with the registry's own destructive flag. No live device is read for this list, so it is
 * the one catalogue the fleet form (no selected machine) can count truthfully.
 */
export type ToolInfo = {
  name: string;
  destructive: boolean;
  description?: string;
};

/**
 * The joins and leaves the console's newest audit window holds for one machine.
 *
 * The durable half of the console-link story: the device's own retry note reaches this console as
 * `last_attempt`, but only while the process that received it lives. The audit file outlives a
 * console restart, so the same link's flapping is still countable. `window` is how many newest lines
 * the counts cover — the lines the read actually returned, not the cap it asked for — served with
 * them so a number cannot read as all time; the field is absent when the window holds no boundary
 * for the machine, never a zero. `span_ms` is the distance between the window's newest and oldest
 * `ts_ms`, so the tally can be read as a rate; it is absent when either end carries no moment, and
 * the page then states the lines without a duration.
 */
export type ChannelActivity = {
  joined: number;
  left: number;
  window: number;
  span_ms?: number | null;
};

export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  last_seen_ms: number | null;
  /**
   * When the console last saw a frame from this device, in Unix milliseconds. `online` is this
   * moment inside the presence window, not the existence of a socket. Absent when the console has
   * seen no frame since it started: a page must not read that as "just now" or as zero.
   */
  last_frame_ms?: number | null;
  /**
   * When the current silence started, from the newest `device.left` that no `device.joined` has
   * closed, in Unix milliseconds. Omitted when the audit's newest window has no such close. It is
   * the newest evidence a console that restarted mid-outage has: there is no frame, and the
   * registry's `last_seen_ms` is the previous channel's open, older than the close.
   */
  last_left_ms?: number | null;
  registered_at_ms?: number | null;
  /**
   * What `/fleet` counted for this machine in the local calendar day: the AI's calls and how many
   * failed. Omitted when the day holds no line naming the machine, so a row draws nothing rather
   * than a zero the read cannot prove. The hub's day line reads the same field with the same rule
   * (`isClientCall`), which is why the two surfaces agree.
   *
   * `since_ms` is present when the audit file starts after local midnight (a rotation, or a console
   * that started later): the count covers the day from that moment, not all of it. `bounded` means
   * the day scan hit its line bound; `scan_limit` names it.
   */
  today?:
    | {
        client_calls: number;
        failed: number;
        since_ms?: number | null;
        bounded?: boolean;
        scan_limit?: number | null;
      }
    | null;
  last_attempt?: { attempts: number; reason: string } | null;
  /**
   * The last absence the audit proves: how long the channel was gone, and when it opened again.
   * Derived from the console's `device.left` / `device.joined` lines, so it survives a console
   * restart; omitted when the newest lines hold no join with a left before it (a device that has
   * only dropped has no completed absence). A page must not read a missing outage as a zero one.
   */
  last_outage?: { away_ms: number; back_at_ms: number } | null;
  /**
   * The audit's join/leave tally over the window the absence pair above is read from. Omitted for a
   * machine with no boundary in that window: the page draws no count rather than a zero.
   */
  channel_activity?: ChannelActivity | null;
  host?: HostFacts | null;
  /**
   * The device's own update story from its newest host frame. Absent for a device that never sent
   * one (an older build); the page draws no line for it rather than inventing a start.
   */
  update?: DeviceUpdate | null;
};
/**
 * One local calendar day as `/audit/day` serves it: the page's four buckets and their failures.
 *
 * `since_ms` is where coverage starts when the file cannot prove the whole day; `bounded` says the
 * scan hit its line bound, and `scan_limit` names it. A page states both instead of presenting a
 * partial day as the whole day.
 */
export type AuditDay = {
  client_calls: number;
  failed: number;
  console: number;
  console_failed: number;
  poll: number;
  poll_failed: number;
  unattributed: number;
  unattributed_failed: number;
  /**
   * **THE MIDNIGHT THIS ANSWER CUT AT (the forty-fourth list).** The answer used to say nothing about
   * the moment its day began, so the page grouped its rows at the BROWSER's midnight — one interval
   * name over two intervals whenever the reader's zone differed from the console host's.
   */
  day_start_ms?: number;
  since_ms?: number | null;
  bounded?: boolean;
  scan_limit?: number | null;
};

/** A range's download: the JSON Lines body and the filename the console chose for it. */
export type AuditExport = { body: string; filename: string };

/**
 * One chosen time range, as `/audit?from=&to=` serves it: `[from, to)`, newest first.
 *
 * The bare array `/audit` has always answered cannot carry the bounds, so a range's answer is this
 * object. `since_ms` is where the current file's coverage starts when it starts after `from`;
 * `bounded` and `scan_limit` name the backwards scan's line bound when it cut the read before the
 * range's start; `at_limit` says the range held more matching lines than the page's window, and
 * `lines` is its newest prefix. Every fact is the server's measurement, never the page's guess.
 */
export type AuditRangeAnswer = {
  lines: AuditLine[];
  lines_read?: number;
  since_ms?: number | null;
  bounded?: boolean;
  scan_limit?: number | null;
  at_limit?: boolean;
};

/**
 * The file the audit window is read from, as `/audit/source` serves it.
 *
 * Every field describes a fact the server could read: the current file's name, size and the moments
 * of its first and last lines, and the one older generation kept beside it. A missing or unreadable
 * file leaves its facts out rather than serving zero, and the page then says nothing at all.
 * `max_bytes` and `rotations` are the writer's configured retention, not a measurement.
 */
export type AuditSource = {
  file?: string;
  bytes?: number;
  oldest_ts_ms?: number;
  newest_ts_ms?: number;
  max_bytes?: number;
  rotations?: number;
  rotated?: { file?: string; oldest_ts_ms?: number };
};

/**
 * The Windows installer the console was given, as `GET /installer` reports it.
 *
 * The version is parsed from the filename (`SummriseSetup-<version>.exe`), never read from the
 * bytes; `size_bytes` and `sha256` describe exactly the file `/installer/download` serves.
 * `available: false` is a normal state — a deployment that staged no installer — not an error.
 */
export type Installer =
  | { available: false }
  | {
      available: true;
      filename: string;
      version: string;
      size_bytes: number;
      sha256: string;
    };

/** Which MCP endpoint a token opens: `/mcp/<device_id>`, or the fleet's `/mcp/all`. */
export type TokenKind = "device" | "fleet";

/**
 * One permanent token's metadata, as the console lists it: never the token itself.
 *
 * The plaintext `srm_…` is returned once by `createToken` and exists nowhere else — the console keeps
 * only its hash, and this list is what the page can show afterwards.
 */
export type TokenMeta = {
  id: string;
  name: string;
  /**
   * The door this token opens. The console writes it on every record. It is optional here because
   * a payload from before the field existed has only the scope shape; `tokenScope` reads that fact
   * and says `unknown` when neither is there, never guessing a door.
   */
  kind?: TokenKind;
  /** Empty on an aggregate token; use `device_ids` for its scope. */
  device_id: string;
  /** The devices an aggregate token may reach; absent on a device token. */
  device_ids?: string[];
  tools: string[];
  allow_destructive: boolean;
  created_at_ms: number;
  /**
   * When this token last authenticated an MCP call, as the console recorded it; absent or `null` for
   * a token that has never been used. The console persists it at most once a minute, so a reload can
   * lag a use by under a minute — the fact is still live in memory on the device.
   */
  last_used_ms?: number | null;
  /**
   * When this token stops authenticating, in Unix milliseconds; absent or `null` for a token created
   * without a lifetime — the default, and how every record written before the field existed reads.
   */
  expires_at_ms?: number | null;
};
/**
 * What a create or a rotation answers with: the record's metadata plus the one plaintext the console
 * ever returns. It is the only shape that carries `token` — the list never does, and the console keeps
 * only the hash afterwards, so a caller has exactly one chance to show it.
 */
export type Created = TokenMeta & { token: string };

export type AuditLine = {
  ts_ms: number;
  /**
   * `mcp` for a call, `poll` for a failed read the console's own poller made (drawn in the table,
   * excluded from every failure count), `device` for a channel boundary, and the operator actions
   * (`grant`, `token`, `auth`, ...). A successful poll is not a line at all.
   */
  kind: string;
  device_id?: string | null;
  /**
   * The record id (64 lowercase hex) of the permanent token that authenticated this call. Absent or
   * `null` for a grant-authenticated call and for every action that is not a token's MCP call; the
   * console never writes the `srm_` value here.
   */
  token_id?: string | null;
  tool?: string | null;
  detail?: string | null;
  status: number;
  /**
   * The console's human sentence for this line, when it wrote one. The live stream always carries
   * it; a fetched window carries it since the console started describing its own index.
   */
  sentence?: string;
  /** How long the device took, when the console measured it (live lines only). */
  ms?: number;
  /**
   * Whether the device's answer was a success, when the console said so. A device's JSON-RPC error
   * is HTTP 200, so the status alone cannot tell a refusal from a result.
   */
  ok?: boolean;
  /** Whether this line arrived on the live stream rather than being read back from the audit file. */
  live?: boolean;
  /**
   * Who made a call the console proxied: `console` for the operator page's own explicit calls and
   * a failed poll, `client` for an AI client on `/mcp/:id` or `/mcp/all`. Absent on an operator
   * action (a grant, a token, a sign-in) and on every line written before the field — neither is a
   * client's call. A successful poll is not written, so no `console` line is one.
   */
  caller?: "console" | "client";
  /** The evidence for this call, when the live stream carried it. */
  evidence?: Record<string, unknown>;
};

/// The console did not answer with JSON: the edge answered instead — an Access login page after the
/// session lapsed, or a proxy's error page. The page reloads once through Access rather than telling
/// the operator to find a token.
export class NoAnswer extends Error {}

class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    /// The console's additive recovery sentence beside the stable machine code. Falls back to the
    /// code, so a caller that only reads `message` still gets something true.
    message?: string
  ) {
    super(message ?? code);
    this.name = "HttpError";
  }
}

async function call<T>(path: string, init: RequestInit = {}): Promise<T> {
  // NO AUTHORIZATION HEADER. The session is the console's HttpOnly cookie, which the browser
  // attaches on its own; a bearer held in a JavaScript variable was lost on every reload and cost
  // the operator the 64-character token again.
  const headers: Record<string, string> = {};
  if (init.body !== undefined) headers["content-type"] = "application/json";
  const response = await fetch(path, { ...init, headers });
  const text = await response.text();
  let body: T | { error?: string } | null;
  try {
    body = text ? (JSON.parse(text) as T | { error?: string }) : null;
  } catch {
    throw new NoAnswer(path);
  }
  if (!response.ok) {
    const failure = body as { error?: string; message?: string } | null;
    const code = failure?.error ?? response.statusText;
    throw new HttpError(response.status, code, failure?.message);
  }
  return body as T;
}

export const api = {
  session: () => call<Session>("/session"),
  signIn: (credential: { token: string } | { code: string }) =>
    call<{ token: string; expires_at_ms: number }>("/auth", {
      method: "POST",
      body: JSON.stringify(credential)
    }),
  // Ends the presented session on the server and, behind Cloudflare Access, names the edge's own
  // logout: clearing only the console cookie is undone by the next request the edge authenticates,
  // so the page follows `access_logout` when it is there. Best effort as the page signs out; a
  // failure is not a reason to keep the in-memory session alive in the tab.
  logout: () => call<{ access_logout: string | null }>("/logout", { method: "POST" }),
  // `/fleet`, not `/devices`: the device namespace is behind Cloudflare Access's bypass (a
  // device has no email login), and a bypassed request carries no identity for the page to use.
  devices: () => call<Device[]>("/fleet"),
  // The device's live catalogue. An empty list is a valid answer (an older build advertises no
  // plugin tools but still has built-ins); 503 means the device is offline, and the body carries the
  // recovery sentence beside its code.
  liveTools: (id: string) =>
    call<{ tools: LiveTool[] }>(`/fleet/${encodeURIComponent(id)}/tools`),
  // `device` narrows the read on the server, during the scan: the newest `limit` lines for that
  // machine, not the fleet's newest `limit` filtered afterwards. Omitting it keeps the fleet window.
  audit: (limit = 20, device?: string) =>
    call<AuditLine[]>(
      `/audit?limit=${limit}${device ? `&device=${encodeURIComponent(device)}` : ""}`
    ),
  // A chosen range, `[from, to)`, answered by the same backwards scan `/audit/day` uses. The
  // page's Time control is the only caller: no range is the newest window above, and a chosen one
  // is this request. `device` narrows the answer, and `limit` is the page's draw size.
  auditRange: (range: { from: number; to: number }, device?: string, limit = 1000) =>
    call<AuditRangeAnswer>(
      `/audit?from=${Math.trunc(range.from)}&to=${Math.trunc(range.to)}&limit=${limit}` +
        `${device ? `&device=${encodeURIComponent(device)}` : ""}`
    ),
  // A chosen range as a file: the same lines `/audit?from=&to=` serves, oldest first, with the
  // console's filename in `Content-Disposition`. This is the one read that is not JSON, so it does
  // not go through `call`: the body is saved as bytes, and a 413's JSON sentence names the bound.
  auditExport: async (
    range: { from: number; to: number },
    device?: string
  ): Promise<AuditExport> => {
    const path =
      `/audit/export?from=${Math.trunc(range.from)}&to=${Math.trunc(range.to)}` +
      `${device ? `&device=${encodeURIComponent(device)}` : ""}`;
    const response = await fetch(path);
    const body = await response.text();
    if (!response.ok) {
      let code = response.statusText;
      let message: string | undefined;
      try {
        const failure = JSON.parse(body) as { error?: string; message?: string };
        code = failure.error ?? code;
        message = failure.message;
      } catch {
        // A proxy's non-JSON refusal keeps the status text as its code.
      }
      throw new HttpError(response.status, code, message);
    }
    const disposition = response.headers.get("content-disposition") ?? "";
    const filename = /filename="([^"]+)"/.exec(disposition)?.[1] ?? "summrise-audit.ndjson";
    return { body, filename };
  },
  // The local calendar day, counted from the file rather than from the newest window. `device`
  // narrows it during the scan, like `/audit?device=`. The hub's `today` and this route are one
  // read; Activity's header uses this so a sliding window cannot shrink the day's number.
  auditDay: (device?: string) =>
    call<AuditDay>(`/audit/day${device ? `?device=${encodeURIComponent(device)}` : ""}`),
  // **WHAT THE WINDOW READS.** The file's name, size, the moments its lines span and its one older
  // generation. A separate read from `/audit` because it is a fact about the reader, not a line.
  auditSource: () => call<AuditSource>("/audit/source"),
  // The release's own installer, when the deployment staged one. The download is a plain
  // same-origin link (`/installer/download`) that sends the session cookie; this read is only the
  // facts that link states, and it is read once rather than polled.
  installer: () => call<Installer>("/installer"),
  // The refusal log is a separate file with the audit line's shape: same route rules, same limit, and
  // no token id because the caller never authenticated.
  // The console's own identity and age. `uptime_s` is the one moment that lets the Machine tab date
  // its missing machine facts by when this process started (the facts live in this process's
  // memory), rather than claiming a device never sent them.
  health: () => call<{ version: string; devices: number; uptime_s?: number }>("/health"),
  // The static registry, with each tool's destructive flag. The console reads it for the grant
  // form: the fleet scope has no selected machine, so this is the only catalogue it can count.
  tools: () => call<ToolInfo[]>("/tools"),
  rename: (id: string, name: string | null) =>
    call<{ id: string; name: string | null }>(`/fleet/${id}/name`, {
      method: "POST",
      body: JSON.stringify({ name })
    }),
  remove: (id: string) => call<void>(`/fleet/${id}`, { method: "DELETE" }),
  connections: (id: string) => call<SavedConnection[]>(`/fleet/${id}/connections`),
  saveConnection: (id: string, draft: Omit<SavedConnection, "id">) =>
    call<SavedConnection>(`/fleet/${id}/connections`, {
      method: "POST",
      body: JSON.stringify(draft)
    }),
  removeConnection: (id: string, connection: string) =>
    call<void>(`/fleet/${id}/connections/${connection}`, { method: "DELETE" }),
  enroll: () => call<{ code: string; expires_at_ms: number }>("/enroll", { method: "POST" }),
  grant: (
    device_id: string,
    tools: string[],
    expires_in_s: number,
    allow_destructive: boolean
  ) =>
    call<{ grant: unknown; signature: string }>("/grants", {
      method: "POST",
      body: JSON.stringify({ device_id, tools, expires_in_s, allow_destructive })
    }),
  // Permanent tokens: the create answer is the only one that carries the plaintext, so it is the one
  // shape that carries `token`. Every list entry is metadata, and a revoke is a bare DELETE.
  tokens: () => call<TokenMeta[]>("/tokens"),
  // There is no narrower `/tokens/<device>` read: the console serves the whole registry and a bare
  // DELETE by id. The one credential list splits the registry by scope, which is the same answer
  // without a request that 405s.
  createToken: (draft: {
    name: string;
    tools: string[];
    allow_destructive: boolean;
    /**
     * The lifetime the new token gets, in seconds: the server accepts 3600 through 31 536 000. Omit
     * it for the default — a token that never expires; the request body then carries no such key.
     */
    expires_in_s?: number;
  } & (
    | { device_id: string; device_ids?: never }
    | { device_ids: string[]; device_id?: never }
  )) =>
    call<Created>("/tokens", {
      method: "POST",
      body: JSON.stringify(draft)
    }),
  // Rotation replaces the record with one of the same scope: the request names only the id, and the
  // answer is the create shape because the new value is returned once, here and nowhere else.
  rotateToken: (id: string) => call<Created>(`/tokens/${id}/rotate`, { method: "POST" }),
  revokeToken: (id: string) => call<void>(`/tokens/${id}`, { method: "DELETE" })
};

export { HttpError };
