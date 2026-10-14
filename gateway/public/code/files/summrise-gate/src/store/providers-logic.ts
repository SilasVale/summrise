/**
 * providers-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `core/summrise-core/src/provider_spec.rs` (`render_typescript`). The freshness test in that module
 * is a byte comparison against this file, so a hand edit fails there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_PROVIDERS=1 cargo test --manifest-path core/summrise-core/Cargo.toml provider_spec
 *
 * `gateway/src/store/providers.ts` re-exports this module and keeps the HOST: the KV reads and writes, the
 * cache, the key resolution, and the `new URL` call. **THE DECISIONS ARE HERE** — the vocabulary, the two
 * prefix helpers, and the parser whose every refusal is a response byte.
 *
 * `parseProviderSpec` TAKES THE PARSED URL, because the core cannot own a WHATWG parser: the host calls
 * `providerBaseText(body)`, parses it, and hands the parts over. **THE REFUSAL ORDER IS UNCHANGED BY THAT**:
 * the host parses before the call and the decision reads `base` only after the prefix, reserved-prefix and
 * label refusals — so a body that is wrong twice gets the refusal the shipping console gives it.
 *
 * **AND IT IMPORTS THE SSRF GUARD RATHER THAN RESTATING IT**: `deviceHostError` is itself an emission (from
 * `gateway/wasm/src/device.rs`), and a second copy of a private-address list is the one duplication in this
 * repository that is a HOLE rather than a bug.
 */

import { deviceHostError } from "../device-fetch.ts";

/** The fields a provider model may declare, in the order the refusal prints them. */
export const MODEL_FIELDS: string[] = [
  "id",
  "name",
  "input",
  "contextWindow",
  "maxTokens",
  "reasoningEffort",
];

/** The `input` values this gateway understands; anything else is refused rather than stored-and-ignored. */
export const KNOWN_MODALITIES: string[] = ["text", "image"];

/** The four reasoning levels the router can speak, in the order the refusal prints them. */
export const REASONING_EFFORTS: string[] = ["low", "medium", "high", "max"];

/** Dialect → the path APPENDED to `baseURL`. One row today; a second is a row here plus its translator. */
export const SUPPORTED_PROVIDER_APIS: Record<string, string> = {
  "openai-completions": "/chat/completions",
};

/** The dialects an operator reaches for first, and WHY this build does not serve them. */
export const KNOWN_UNSUPPORTED_APIS: Record<string, string> = {
  "anthropic-messages":
    "this gateway has no OpenAI→Anthropic request translator, so the provider would serve /v1/messages and fail every OpenAI-format client",
};

/** Every prefix a CUSTOM provider may not claim, **in the order the 409 prints them**. */
export const RESERVED_PREFIXES: string[] = [
  "og",
  "or",
  "nv",
  "gmi",
  "qw",
  "cm",
  "r4",
  "none",
  "ds",
  "amd",
];

/** One model of a custom provider. */
export interface ProviderModel {
  /** The WIRE name — what the upstream is asked for. */
  id: string;
  /** Display name, advertised on /v1/models. */
  name?: string;
  /** DSH `input: [text, image]`: the model sees images itself, so gateway-side vision preprocessing must stay
   *  OUT of the way. */
  vision?: boolean;
  /** Declared limits, advertised on /v1/models as context_window/max_tokens. */
  contextWindow?: number;
  maxTokens?: number;
  /** Default reasoning effort when the client sends none. */
  reasoningEffort?: "low" | "medium" | "high" | "max";
}

/** A custom provider record (one `providers:custom` entry). */
export interface ProviderSpec {
  /** Routing prefix WITH its trailing slash — the ROUTE_INFO spelling ("my/"). */
  prefix: string;
  /** Console/route-card name. Defaults to the prefix without its slash. */
  label: string;
  /** Upstream root; the dialect's path is APPENDED to it. */
  baseURL: string;
  /** Wire dialect — a key of SUPPORTED_PROVIDER_APIS. */
  api: string;
  /** NAME of a Worker env binding/secret holding the key. */
  apiKeyEnv?: string;
  /** Inline key. NEVER returned by the admin API. */
  apiKey?: string;
  models: ProviderModel[];
}

/** Why a provider record was refused, and with which HTTP status. 400 = wrong; 409 = a reserved prefix. */
export interface ProviderParseError {
  error: string;
  status: number;
}

/** The URL's parsed parts, as the host's parser produced them. `protocol` carries its colon. */
export interface ParsedBase {
  protocol: string;
  username: string;
  password: string;
  search: string;
  hash: string;
  hostname: string;
  origin: string;
  pathname: string;
}

/** `barePrefix(prefix)` — the TRAILING slashes come off. `?? ""` is a COERCION, not a null check: a number
 *  becomes its digits and an object becomes `[object Object]`. */
export function barePrefix(prefix: unknown): string {
  const text = prefix === null || prefix === undefined ? "" : String(prefix);
  return text.replace(/\/+$/, "");
}

/** The advertised id of a wire model under a prefix — prefix ONCE, never twice. */
export function advertisedModelId(prefix: string, wire: unknown): string {
  const bare = barePrefix(prefix);
  const w = (wire === null || wire === undefined ? "" : String(wire)).replace(/^\/+/, "");
  return w.startsWith(bare + "/") ? w : `${bare}/${w}`;
}

/** `String(body.baseURL ?? "").trim()` — the text the host's URL parser is handed. One owner, because a second
 *  coercion in the host would be the twin this module exists to delete. */
export function providerBaseText(body: any): string {
  return String(body?.baseURL ?? "").trim();
}

/** One entry of `models`, or the message the route answers with. */
export function parseProviderModel(
  raw: any,
  at: number,
): { model?: ProviderModel; error?: string } {
  const where = `models[${at}]`;
  if (typeof raw === "string") {
    const id = raw.trim();
    return id ? { model: { id } } : { error: `${where}: empty model id` };
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return { error: `${where} must be a model id string or an object` };
  }
  const unknown = Object.keys(raw).filter((k) => !MODEL_FIELDS.includes(k));
  if (unknown.length) {
    return {
      error: `${where}: unsupported field(s) ${unknown.join(", ")} — this gateway stores ${MODEL_FIELDS.join(", ")}`,
    };
  }
  const id = String(raw.id ?? "").trim();
  if (!id) return { error: `${where}: id is required` };
  if (/\s/.test(id)) return { error: `${where}.id must not contain whitespace` };
  const model: ProviderModel = { id };
  if (raw.name !== undefined) {
    const name = String(raw.name).trim();
    if (name) model.name = name;
  }
  if (raw.input !== undefined) {
    if (!Array.isArray(raw.input)) {
      return { error: `${where}.input must be an array of ${KNOWN_MODALITIES.join("/")}` };
    }
    const bad = raw.input.filter((m: any) => !KNOWN_MODALITIES.includes(String(m)));
    if (bad.length) {
      return {
        error: `${where}.input: unsupported modalit${bad.length > 1 ? "ies" : "y"} ${bad
          .map((b: any) => JSON.stringify(String(b)))
          .join(", ")} — only ${KNOWN_MODALITIES.join(" and ")} are understood`,
      };
    }
    // A model that declares image input must NOT have its images described by the gateway's vision model
    // (preprocessImages) — it can see them itself.
    if (raw.input.map(String).includes("image")) model.vision = true;
  }
  // The reasoning DEFAULT, on the same terms as a console-added record: a level the router cannot speak is
  // refused BY NAME here rather than dropped, because a typo that silently means "no default" is a behaviour
  // change nobody can see.
  if (raw.reasoningEffort !== undefined) {
    const lv = String(raw.reasoningEffort).trim();
    if (!REASONING_EFFORTS.includes(lv))
      return { error: `${where}.reasoningEffort must be one of ${REASONING_EFFORTS.join(", ")}` };
    model.reasoningEffort = lv as "low" | "medium" | "high" | "max";
  }
  for (const cap of ["contextWindow", "maxTokens"] as const) {
    const v = raw[cap];
    if (v === undefined) continue;
    const n = Number(v);
    if (!Number.isInteger(n) || n <= 0) {
      return { error: `${where}.${cap} must be a positive integer` };
    }
    model[cap] = n;
  }
  return { model };
}

/** The parser. Pure — the admin handler only has to map the error onto a status. */
export function parseProviderSpec(
  body: any,
  base: ParsedBase | null,
  opts: { keepKeyFrom?: { apiKey?: string; apiKeyEnv?: string } } = {},
): { spec?: ProviderSpec } & Partial<ProviderParseError> {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    return { error: "body must be a JSON object", status: 400 };
  }
  // Lowercase only: routing derives the prefix by splitting the CLIENT's model id on "/", so a mixed-case
  // prefix could never match a lowercase one.
  const prefix = String(body.prefix ?? "").trim();
  if (!/^[a-z0-9][a-z0-9-]{0,30}\/$/.test(prefix)) {
    return {
      error: `prefix must be a lowercase channel prefix ending in "/" (e.g. "my/") — got ${JSON.stringify(prefix)}`,
      status: 400,
    };
  }
  const bare = barePrefix(prefix);
  if (RESERVED_PREFIXES.includes(bare)) {
    return {
      error: `prefix ${JSON.stringify(prefix)} is reserved by a built-in channel (reserved: ${RESERVED_PREFIXES.join(", ")})`,
      status: 409,
    };
  }
  const label = String(body.label ?? "").trim() || bare;
  // eslint-disable-next-line no-control-regex
  if (label.length > 80 || /[\u0000-\u001f\u007f]/.test(label)) {
    return {
      error: "label must be at most 80 characters, with no control characters",
      status: 400,
    };
  }
  const rawBase = providerBaseText(body);
  if (!base) {
    return { error: `baseURL is not a URL — got ${JSON.stringify(rawBase)}`, status: 400 };
  }
  const url = base;
  if (url.protocol !== "https:") {
    return {
      error: `baseURL must be https (got ${url.protocol}//) — a provider key must never travel in clear`,
      status: 400,
    };
  }
  if (url.username || url.password) {
    return { error: "baseURL must not embed credentials", status: 400 };
  }
  if (url.search || url.hash) {
    return { error: "baseURL must not carry a query or a fragment", status: 400 };
  }
  // The SSRF check, REUSED (device-fetch.ts) — loopback/private/link-local/metadata hosts are refused here for
  // the same reason a device hostname is: the gateway would dial them with a credential attached.
  const hostErr = deviceHostError(url.hostname);
  if (hostErr) return { error: `baseURL host rejected: ${hostErr}`, status: 400 };
  // Treated as a PREFIX, not resolved against (see providerRoute): a deployment path such as
  // https://host/openai/v1 must survive.
  const baseURL = (url.origin + url.pathname).replace(/\/+$/, "");
  const api = String(body.api ?? "").trim();
  if (!Object.prototype.hasOwnProperty.call(SUPPORTED_PROVIDER_APIS, api)) {
    const why = KNOWN_UNSUPPORTED_APIS[api];
    return {
      error:
        `unsupported protocol ${JSON.stringify(api)} — this gateway serves ` +
        `${Object.keys(SUPPORTED_PROVIDER_APIS).join(", ")}${why ? ` (${api}: ${why})` : ""}`,
      status: 400,
    };
  }
  const apiKeyEnv = String(body.apiKeyEnv ?? "").trim();
  const apiKey = String(body.apiKey ?? "").trim();
  if (apiKeyEnv && apiKey) {
    return {
      error:
        "set apiKeyEnv (a deployed Worker secret) or apiKey (inline), not both — two key sources cannot both be the one this provider sends",
      status: 400,
    };
  }
  // AN UPDATE MAY OMIT THE KEY; A CREATE MAY NOT: `POST /api/admin/providers` is the edit path for a record the
  // console owns, and the admin view never returns the key, only `keyMasked`. The caller passes what the STORED
  // record already has, and only for an existing prefix.
  const keep = !apiKeyEnv && !apiKey ? opts.keepKeyFrom : undefined;
  if (!apiKeyEnv && !apiKey && !keep) {
    return {
      error:
        "a key is required: apiKeyEnv (the NAME of a Worker secret, DSH's settings.yaml spelling) or apiKey (an inline value stored in KV)",
      status: 400,
    };
  }
  if (apiKeyEnv && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(apiKeyEnv)) {
    return {
      error: `apiKeyEnv must be a Worker env/secret NAME (letters, digits, "_" — e.g. SUMMRISE_API_KEY), not a value — got ${JSON.stringify(apiKeyEnv)}`,
      status: 400,
    };
  }
  if (apiKey && (apiKey.length < 8 || /\s/.test(apiKey))) {
    // <8 also keeps the credential out of redactSecrets' "too generic to replace safely" bucket, so an
    // upstream echo of it is still redacted.
    return { error: "apiKey must be at least 8 characters with no whitespace", status: 400 };
  }
  if (!Array.isArray(body.models) || body.models.length === 0) {
    return { error: "models must be a non-empty array", status: 400 };
  }
  if (body.models.length > 200) {
    return { error: `models: at most 200 entries (got ${body.models.length})`, status: 400 };
  }
  const models: ProviderModel[] = [];
  for (let i = 0; i < body.models.length; i++) {
    const { model, error } = parseProviderModel(body.models[i], i);
    if (error || !model) return { error: error || `models[${i}] is invalid`, status: 400 };
    // Normalize to the WIRE form: the full-id spelling (prefix included) is accepted, stored once, and never
    // advertised twice.
    const wire = model.id.startsWith(bare + "/") ? model.id.slice(bare.length + 1) : model.id;
    if (!wire) return { error: `models[${i}]: empty model id after the prefix`, status: 400 };
    if (models.some((m) => m.id === wire)) {
      return {
        error: `models[${i}]: duplicate model id ${advertisedModelId(bare, wire)}`,
        status: 400,
      };
    }
    models.push({ ...model, id: wire });
  }
  const spec: ProviderSpec = { prefix, label, baseURL, api, models };
  if (apiKeyEnv) spec.apiKeyEnv = apiKeyEnv;
  else if (apiKey) spec.apiKey = apiKey;
  // The stored key, carried forward because the body omitted it — never a synthesis: an update that DOES send
  // one replaces it, which is how key rotation must keep working.
  else if (keep?.apiKeyEnv) spec.apiKeyEnv = keep.apiKeyEnv;
  else if (keep?.apiKey) spec.apiKey = keep.apiKey;
  return { spec };
}
