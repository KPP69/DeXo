import { invoke } from "@tauri-apps/api/core";

/** Startup status reported by the Rust backend (see `app_status` command). */
export interface AppStatus {
  version: string;
  dataDir: string;
  databasePath: string;
  databaseReady: boolean;
}

export async function fetchAppStatus(): Promise<AppStatus> {
  return invoke<AppStatus>("app_status");
}
