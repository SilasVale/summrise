# The model catalogue's file layer

Three layers, in precedence order:

| layer | lives in | changed by | wins over |
|---|---|---|---|
| **file** (this directory) | `models.ts`, in git | commit + `wrangler deploy` | everything |
| **console** | KV (`providers:custom`, `models:custom`, `models:overrides`) | the Models page | the built-in registry's display facets |
| **registry** | `src/channels.ts` | code + deploy | — (routing facets are pinned here) |

## What may be declared

- `providers[]` — a whole custom provider (`prefix`, `baseURL`, `api`, `apiKeyEnv`, `models[]`).
  Validated by the SAME parser the console uses (`parseProviderSpec`), so a file cannot
  express something the API would refuse.
- `models[]` — a model on a built-in channel (`id` plus the display facets).
- `overrides[]` — display facets for a BUILT-IN id (`name`, `contextWindow`, `maxTokens`,
  `reasoningEffort`). **Routing fields are refused by name**, exactly as in the console: a
  file must not be a second, contradictory source of truth about where a request goes.

## Why a malformed file fails the deploy

`src/store/file-config.ts` parses this file at MODULE LOAD with the same validators the
admin routes use. A typo therefore fails `wrangler deploy` — it cannot reach production and
be discovered by a model that suddenly 502s.

## The one thing to be careful about

**The file wins.** An entry declared here overrides the console's version of the same id or
prefix, so editing that entry in the panel is pointless until the file stops declaring it —
which is why the panel marks file-declared entries instead of letting the edit look applied.
