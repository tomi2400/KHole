import { invoke } from "@tauri-apps/api/core";

/** Typed wrappers around the Rust commands (src-tauri/src/commands.rs). */

export interface AppInfo {
  version: string;
  dbPath: string;
  schemaVersion: number;
}

export interface AnalyzerInfo {
  name: string;
  version: string;
  essentia: string;
}

export const api = {
  ping: () => invoke<string>("ping"),
  appInfo: () => invoke<AppInfo>("app_info"),
  analyzerInfo: () => invoke<AnalyzerInfo>("analyzer_info"),
};
