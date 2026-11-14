export function startLegacyPanel(options: { deviceBase: string; html: string }): Promise<{
  base: string;
  close: () => Promise<void>;
}>;
