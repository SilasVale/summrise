/**
 * Copy text with the operator's clipboard, falling back to the selection trick.
 *
 * `navigator.clipboard` needs a secure context and a permission; a console reached over plain HTTP on
 * a LAN address has neither. The textarea-and-execCommand path is what every browser still honours,
 * and it is the same code the diagnostics button, the AI-client card and the memory export used to
 * carry separately.
 *
 * Returns whether the text reached the clipboard. A caller that says "Copied" must only say it when
 * this answers true: a failed copy that reports success is worse than no button.
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    document.body.append(area);
    try {
      area.select();
      return document.execCommand("copy");
    } catch {
      // Some browsers throw instead of answering false; a caller that says "Copied" only on a true
      // answer must not receive a rejection it does not catch.
      return false;
    } finally {
      area.remove();
    }
  }
}
