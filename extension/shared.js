// shared.js — the single copy of constants and guards shared by the content
// script (content/studio-links.js) and the options page (options/options.js).
// Load order matters: it must run BEFORE both (first entry of the manifest
// content_scripts js array / first <script> in options.html). It must not
// touch chrome.* at load time — content scripts run in an isolated world
// where these top-level bindings are the cross-file channel.

const DEFAULT_STUDIO_ORIGIN = "https://vscode.saisi.online";

// THE TWO ROOTS, and why a chat message is not allowed to pick either.
//
// STUDIO_ROOT is the folder code-server was started on (`--auth none
// /home/zhengsaisi`): everything it can open lives underneath it. A mention that
// resolves outside it is refused, because the link would be a 404 at best.
//
// DEFAULT_WORKSPACE is the directory DSH actually runs in (its session cwd):
// a RELATIVE mention like `gateway/src/index.ts` means a path in the PROJECT,
// not a path in the user's home. It used to be the home directory, so every
// relative mention in every reply linked to a folder that does not exist
// (`/home/zhengsaisi/gateway/src`).
const STUDIO_ROOT = "/home/zhengsaisi";
const DEFAULT_WORKSPACE = "/home/zhengsaisi/vale";

// A DOT-SEGMENT IS A CREDENTIAL HOME UNLESS IT IS THIS ONE — and `..` is a dot
// segment, so this ONE rule also refuses traversal (a `..` never survives to be
// normalised). It is deliberately one rule in one place: an earlier draft had a
// separate `..` check, and a mutation proved nothing could fail it, because this
// rule already caught every input it did.
//
// The allowlist above does NOT make `.ssh` safe: it is inside the home
// directory, so `/home/zhengsaisi/.ssh/id_rsa` in a chat message used to become a
// one-click link into the key directory of the already-authenticated IDE session
// — and the same for `.dsh`, `.aws`, `.gnupg`. `.github` is the one dot-directory
// that is project content rather than a credential store, so it is the one that
// stays linkable. New credential homes are refused by default; naming the
// exceptions is what keeps that true.
const LINKABLE_DOT_DIRS = [".github"];

// Normalize to a bare https:// origin (path/query dropped), or null when the
// value is not a well-formed https:// URL. Canonical guard: the code-server
// session rides on browser cookies — and THE COOKIES ARE THE ONLY GATE, which is
// what makes this check load-bearing rather than tidy (round 141, X8). The live
// server runs `--auth none` (pm2 args: ["--auth","none","/home/zhengsaisi"]), so
// there is NO code-server password; Cloudflare Access is the single gate in front
// of it. An earlier version of this comment said "Access + code-server password",
// which named a credential that does not exist and read as defence-in-depth where
// there is none. So the origin must never be a cleartext http:// URL (an Access
// cookie over http is the whole door) or something that isn't a URL at all.
function httpsOrigin(v) {
  try {
    const u = new URL(v);
    return u.protocol === "https:" ? u.origin : null;
  } catch {
    return null;
  }
}

// Path-mention → folder resolution (SOLID Round-96: verbatim core of the
// content script's resolve(), minus its TTL cache — cache lifetime belongs
// to the page, this mapping is pure). Absolute paths map directly (a file
// mention yields its folder); relative paths join onto the workspace base.
// Best-effort by design: code-server opens the folder even when the file
// does not exist.
function resolveDir(raw, base) {
  const root = base || DEFAULT_WORKSPACE;
  const mention = String(raw || "").replace(/^\.\//, "");
  if (!mention) return "";
  const joined = mention.startsWith("/") ? mention : `${root}/${mention}`;
  const flat = joined.replace(/\/{2,}/g, "/").replace(/\/$/, "");

  // 1. STRICTLY inside the folder code-server can open. The root itself is refused
  //    too: a mention's folder is a place in the project, not the whole home
  //    directory listing.
  if (!flat.startsWith(`${STUDIO_ROOT}/`)) return "";

  // 2. not inside a credential home? (see LINKABLE_DOT_DIRS)
  const rel = flat.slice(STUDIO_ROOT.length).split("/").filter(Boolean);
  for (const seg of rel) {
    if (seg.startsWith(".") && !LINKABLE_DOT_DIRS.includes(seg)) return "";
  }

  // 3. a file mention yields its FOLDER; a folder mention stays itself. Only a
  //    dot in the last segment is treated as a file: an extensionless file
  //    (`Makefile`) still resolves to itself, which opens its parent directory —
  //    recorded as a known limit rather than guessed at with a name list.
  let dir = flat;
  const last = dir.slice(dir.lastIndexOf("/") + 1);
  if (!mention.endsWith("/") && last.includes(".")) dir = dir.slice(0, dir.lastIndexOf("/"));
  return dir.replace(/\/+$/, "") || STUDIO_ROOT;
}

// code-server opens folders: /?folder=<abs>. The line number cannot ride in
// the URL (VS Code web limitation) — callers put it in the tooltip.
function studioFolderUrl(origin, dir) {
  return `${origin}/?folder=${encodeURIComponent(dir)}`;
}

// Path-mention matcher (SOLID Round-96: verbatim core of the content
// script's per-node scan). A trailing :NN line number is part of the match;
// short (<4 chars) and extensionless-relative mentions are noise, skipped.
//
// "RELATIVE" MEANS: IT LOOKS LIKE A FILE. The relative arm used to accept any
// `word/word`, so ordinary prose became links — `either read/write or and/or
// him/her` produced three of them. A relative mention now has to end in a known
// extension (or carry a line number, which prose does not); an absolute mention
// still only has to be rooted, because `/a/b` in a sentence is a path.
const PATH_RX =
  /(?:(?<![\w.\/-])(?:\/[A-Za-z0-9_.~-]+)+|(?:[A-Za-z0-9_.~-]+\/)+[A-Za-z0-9_.-]+\.(?:tsx?|mjs|cjs|jsx|rs|c|h|cpp|hpp|json|ya?ml|toml|md|sh|py|css|scss|html|sql|ini|conf|lock)|[A-Za-z0-9_.-]+\.(?:tsx?|mjs|cjs|jsx|rs|c|h|cpp|hpp|json|ya?ml|toml|md|sh|py|css|scss|html|sql|ini|conf|lock))(?::\d+)?/g;

function extractPathJobs(text) {
  PATH_RX.lastIndex = 0;
  const jobs = [];
  let m;
  while ((m = PATH_RX.exec(text))) {
    const raw = m[0];
    if (raw.length < 4) continue;
    if (!raw.includes("/") && !raw.slice(1).includes(".")) continue;
    const cm = raw.match(/:(\d+)$/);
    const bare = cm ? raw.slice(0, raw.length - cm[0].length) : raw;
    jobs.push({ raw, bare, lineNo: cm ? Number(cm[1]) : 0, index: m.index });
  }
  return jobs;
}

// Test seam (SOLID Round-24): expose the pure guards to node --test without
// changing browser semantics — classic <script> pages have no `module`, so
// this block is inert there; the extension CI job runs extension/test/.
if (typeof module !== "undefined" && module.exports) {
  module.exports = {
    DEFAULT_STUDIO_ORIGIN,
    STUDIO_ROOT,
    DEFAULT_WORKSPACE,
    httpsOrigin,
    resolveDir,
    studioFolderUrl,
    extractPathJobs,
  };
}

