// lib/recipe.ts — turn a walked path into a RECIPE.
//
// This is beat 6 of the design's core loop (docs/adr/proposal-game-design.md §2):
// "HARVEST — the path becomes a recipe; next run reuses it". The six-beat table
// records it as HALF-done, with the gap stated as "no recipe". This closes it.
//
// WHY DEVICE MEMORY IS THE RIGHT STORE, not a new table:
//
//   * Recipes land in the memory plugin, which is ALREADY shared across every AI
//     client on the device. That is what makes a recipe re-runnable without any
//     protocol change: the operator saves it here, and an AI client can find it
//     with the memory_search tool it already has. A private panel-only store
//     would be a recipe the AI cannot see — i.e. one nobody can actually run.
//   * The memory store already sanitizes credential-shaped values on write, so a
//     recipe cannot become a place secrets leak from.
//   * No new Rust, no new endpoint, no schema migration.
//
// WHAT A RECIPE IS, AND IS NOT. It is a durable record of a sequence that was
// actually walked on this device, with the outcome it had at the time. It is NOT
// an executable script and this module does not pretend otherwise: re-running it
// still goes through the AI (or the operator), because automatic re-execution is
// the control plane's job (proposal-control-path.md), which is a separate and
// much larger piece of work. The design's own rule applies — say what is true.
//
// ── RUST SINCE 2026-09-30 (block ②) ──────────────────────────────────────────────────────────────
//
// All three functions are `agent/resources/panel-logic/src/recipe.rs` now, and the differential is
// **682 corpus cases with 0 divergences** — values, KEY ORDER and whether each side raised — with
// every arm reached. `components/__tests__/recipe.test.tsx` (412 lines, the PathView save flow
// included) runs UNCHANGED against it.
//
// THE TWO CONSTANTS BELOW STAY, and they are passed INTO the crate: they are the recipe format's
// VOCABULARY — what a client greps the shared store for — and this module's own test imports them.
// It is the split `liveness.rs` records for `WORKING_MS`: the surface owns the words.
//
// AND THREE FINDINGS FROM THE DIFFERENTIAL ARE WORTH KNOWING WHEN THIS FILE IS READ:
//
//   * **A NULLISH PROPERTY READ RAISES.** `path.summary.steps` on a path with no summary is a
//     TypeError in the TypeScript, and the first port answered "undefined steps" — writing a recipe
//     the JavaScript refuses to write.
//   * **`st.considered.join` IS A METHOD CALL**, so a `considered` that is a non-empty string raises
//     where the first port answered an empty join.
//   * **THE TITLE AND THE CONTENT ARE BUILT IN JS**, not in a Rust `String`: `slice(0, 45)` counts
//     UTF-16 units and can cut a surrogate pair in half, and a lone surrogate cannot exist in UTF-8.
//     `runs.rs` names that same boundary for a React list key and rounds down; here the string is
//     RENDERED and written to the device's memory, so it is exact.
import { logic } from "../wasm/panelLogic";
import type { SessionPath, PathStep } from "./path";

/** Marker line so a recipe is identifiable in the shared store, and greppable
 *  by a client that only has the raw text. */
export const RECIPE_MARKER = "summrise-recipe/v1";

/** Tag every recipe carries, so `memory_search` with tags finds them. */
export const RECIPE_TAG = "recipe";

interface RecipeDraft {
  /** Short title — becomes the memory entry title. */
  title: string;
  /** The entry body. */
  content: string;
  tags: string[];
}

/** Title shown in the save form, derived from the path so the operator usually
 *  only has to confirm it. Uses the FIRST command (what the run was about) and
 *  the step count. */
export function suggestedTitle(path: SessionPath): string {
  return logic().suggested_title(path) as string;
}

interface RecipeInput {
  name: string;
  /** Which session this was walked on — shell kind and label, so a reader knows
   *  what the commands were run against. */
  sessionLabel?: string;
  sessionKind?: string;
  /** What the session was asked to achieve. A recipe without its purpose is a
   *  list of commands someone has to reverse-engineer — and the purpose is the
   *  one thing that cannot be recovered from the commands themselves. */
  goal?: string | null;
}

/**
 * Render the recipe body.
 *
 * The shape is deliberate: a machine-readable marker line, the outcome summary
 * (so a recipe that half-failed is honest about it rather than presenting itself
 * as a known-good procedure), then the commands one per line in order.
 */
export function buildRecipe(path: SessionPath, input: RecipeInput): RecipeDraft {
  return logic().build_recipe(
    path,
    input,
    RECIPE_MARKER,
    RECIPE_TAG,
  ) as RecipeDraft;
}

/** Steps that make a recipe questionable — surfaced in the save form so the
 *  operator is not silently saving a broken procedure as a good one. */
export function recipeWarnings(steps: PathStep[]): string[] {
  return logic().recipe_warnings(steps) as string[];
}
