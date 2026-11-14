export function startLegacyPanel(options: {
  deviceBase: string;
  html: string;
  desktopDir?: string;
  token?: string;
  activityHtml?: string;
  capture?: () => Promise<string>;
}): Promise<{
  base: string;
  close: () => Promise<void>;
}>;
