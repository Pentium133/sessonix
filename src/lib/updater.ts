import { openUrl } from "@tauri-apps/plugin-opener";

export async function openReleasePage(url: string): Promise<void> {
  await openUrl(url);
}
