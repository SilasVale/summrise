import { recoveryStory, type RecoveryDevice } from "./recovery";

/**
 * The muted sentence beside the device's start: `back after 14 h 57 m`, or why the console cannot
 * say. It states a gap only when the console's channel pair or the device's own boot journal proves
 * one; a machine with neither record draws nothing (audit-44 F12).
 *
 * `consoleStartedAt` is when this console process began, from `/health`'s uptime. Inside the gap it
 * means the console's own downtime is part of the silence, so the duration is withheld and the line
 * says the console cannot vouch for it.
 */
export function RecoveryLine({
  device,
  consoleStartedAt
}: {
  device?: RecoveryDevice | null;
  consoleStartedAt?: number | null;
}) {
  const story = recoveryStory(device, consoleStartedAt);
  if (!story) return null;
  return (
    <p className="recovery-line" data-recovery-source={story.source} title={story.title ?? undefined}>
      {story.text}
    </p>
  );
}
