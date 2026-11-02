/**
 * What the device still has of a session's output.
 *
 * The device keeps the newest 1 MiB (`terminal::BUFFER_BYTES`) and the cursor is an absolute byte
 * count. A tab whose cursor fell behind the ring is refused, and the refusal names where the retained
 * output starts; the export reads from 0 and, only on that refusal, retries from there — saying in the
 * file how many bytes are gone instead of calling the transcript the whole session.
 */
export const RETAINED_BYTES = 1024 * 1024;

/** The start of the retained output when the sentence is the device's too-old refusal, else null. */
export function earliestCursor(sentence: string): number | null {
  const match = /the cursor \d+ is older than the retained output, which starts at (\d+)/.exec(sentence);
  return match ? Number(match[1]) : null;
}

export type TranscriptRead = (cursor: number) => Promise<{ data: string; cursor: number }>;

export type Transcript = { text: string; from: number; to: number; dropped: number };

/**
 * Read the session's retained output, recovering once from a cursor older than the ring.
 *
 * The first read is at 0. Only a refusal that names the earliest retained byte earns a retry — any
 * other failure is not about the cursor and propagates unchanged.
 */
export async function readTranscript(read: TranscriptRead): Promise<Transcript> {
  try {
    const answer = await read(0);
    return { text: answer.data, from: 0, to: answer.cursor, dropped: 0 };
  } catch (cause) {
    const earliest = earliestCursor(cause instanceof Error ? cause.message : String(cause));
    if (earliest === null) throw cause;
    const answer = await read(earliest);
    return { text: answer.data, from: earliest, to: answer.cursor, dropped: earliest };
  }
}

/** The transcript as one file: what was read, from where, and what the ring no longer has. */
export function transcriptFile(transcript: Transcript, meta: { session: string; at: Date }): string {
  const dropped =
    transcript.dropped === 0
      ? "dropped: none — the whole session is retained"
      : `dropped: ${transcript.dropped} bytes — older output is no longer retained and cannot be recovered`;
  return [
    "Summrise terminal transcript",
    `session: ${meta.session}`,
    `exported: ${meta.at.toISOString()}`,
    `range: bytes ${transcript.from}..${transcript.to} of this session's output`,
    `retained: the device keeps the newest ${RETAINED_BYTES / (1024 * 1024)} MiB of a session's output`,
    dropped,
    "---",
    transcript.text
  ].join("\n");
}
