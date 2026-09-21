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

/** A DIAL'S TONE, from "how many of N are well" — the same question the channels tile and the devices tile both ask (round
 *  132). They answered it differently: channels showed `warn` when some were down, devices showed `ok` while a device was
 *  down, because its expression had no middle term. A count that is SOME-but-not-all healthy is not "ok", and the state
 *  layer exists so that sentence is true on every surface.
 *
 *  `known` is separate from `total` because "we have not asked yet" is not "none are healthy" — the same distinction
 *  `deviceState.ts` records for a probe that has not answered, and the reason this takes three numbers rather than two. */
export type DialTone = "ok" | "warn" | "off";

export function healthTone(known: boolean, ok: number, total: number): DialTone {
  if (!known || total === 0) return "off";
  return ok === total ? "ok" : ok > 0 ? "warn" : "off";
}
