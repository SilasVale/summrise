/**
 * Save bytes as a file: a Blob and a temporary anchor, with no console route in between.
 *
 * Every download in the console is the same gesture — the device answered with bytes and the page
 * hands them to the browser. The object URL is revoked after a second, once the browser has had time
 * to start the download.
 */
export function downloadFile(name: string, body: BlobPart, type: string): void {
  const url = URL.createObjectURL(new Blob([body], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  // Give the browser a moment to start the download before the URL is revoked.
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
