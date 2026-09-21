// channelState.ts — IS THIS PROVIDER CHANNEL HEALTHY? ONE derivation, and the LABEL belongs with it.
//
// WHY (round 131 of the standing goal). One row rendered the same fact three times in three consecutive lines:
//
//     <span className={`dot ${c.ok ? "ok" : "err"}`} />
//     <span className={`health-state ${c.ok ? "ok" : "err"}`}>
//       {c.ok ? t("overview.healthOk") : c.reason || t("overview.healthDown")}
//
// That is the spine's first clause with three spellings of one answer, and it is what `lib/deviceState.ts` was written to
// end for the DEVICE's signal. A channel is a different fact — it is a provider's health, not a device's reachability — so
// it goes here rather than there, with the same shape: the signal and the words that go with it, decided once.
//
// WHAT IT DOES NOT DECIDE: how the row lays out, when the section renders at all, or the roll-up count (channels healthy /
// channels total), which is a different question the view asks on its own.
type ChannelSignal = "ok" | "err";

/** The signal for a channel. `ok` is the provider's own answer; anything else is a failure with a reason the row shows. */
export function channelSignal(ok: boolean): ChannelSignal {
  return ok ? "ok" : "err";
}

/** The words for that signal, given the provider's reason when it has one. The `reason` wins over the generic line because a
 *  provider that says WHY is more useful than a label that says WHAT. */
export function channelLabel(
  c: { ok: boolean; reason?: string },
  t: (key: string) => string,
): string {
  return c.ok ? t("overview.healthOk") : c.reason || t("overview.healthDown");
}
