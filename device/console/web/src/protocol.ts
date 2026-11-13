// The console <-> device wire, generated from `protocol/src/lib.rs` by ts-rs.
//
// Run `cargo test -p summrise-protocol export_bindings` after changing a protocol type; the files
// under `./protocol/` are rewritten, not hand-edited. `scripts/check-protocol-ts.mjs --check` fails
// when a committed binding is stale.
export type { Answer } from "./protocol/Answer";
export type { Body } from "./protocol/Body";
export type { EventsSubscribe } from "./protocol/EventsSubscribe";
export type { Frame } from "./protocol/Frame";
export type { Subscribe } from "./protocol/Subscribe";
export type { TokenScope } from "./protocol/TokenScope";
export type { Unsubscribe } from "./protocol/Unsubscribe";
