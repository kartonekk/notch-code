import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export type Phase = "preparing" | "ready" | "recording" | "error";

export interface AppStatus {
  phase: Phase;
  message: string;
  /** 0..1 during model download, null otherwise */
  progress: number | null;
  /** e.g. "GPU (Vulkan)" or "CPU" once the model is loaded */
  backend: string | null;
  /** source name while recording */
  source: string | null;
  /** unix ms when recording started */
  started_at: number | null;
  transcript_path: string | null;
  /** recording is on hold (session and file stay open) */
  paused: boolean;
}

export interface AudioSource {
  pid: number;
  name: string;
  exe: string;
  active: boolean;
  peak: number;
  suggested: boolean;
}

export type Speaker = "me" | "others";

export interface Segment {
  id: number;
  speaker: Speaker;
  text: string;
  /** ms since recording start */
  start_ms: number;
  final: boolean;
}

export interface Levels {
  app: number;
  mic: number;
}

export interface NetStatus {
  online: boolean;
  /** TCP connect time to a public host, null when offline */
  latency_ms: number | null;
}

export type ClaudeAuthState =
  | "checking"
  | "installing"
  | "not_installed"
  | "signed_out"
  | "signing_in"
  | "signed_in"
  | "error";

export interface ClaudeStatus {
  state: ClaudeAuthState;
  /** what to tell the user, e.g. why the check failed */
  message: string | null;
  /** subscription reported by the CLI ("pro", "max", …) */
  plan: string | null;
  /** a background sign-in didn't finish: offer the visible sign-in window */
  suggest_window: boolean;
}

export interface UpdateStatus {
  state: "idle" | "available" | "downloading" | "error";
  /** build number of this exe; null for local builds (they're offered the newest CI build) */
  current: number | null;
  /** build number of the newer release */
  available: number | null;
  /** 0..1 while downloading */
  progress: number | null;
  message: string | null;
}

export const api = {
  status: () => invoke<AppStatus>("get_status"),
  sources: () => invoke<AudioSource[]>("list_sources"),
  start: (pid: number, includeMic: boolean) =>
    invoke<void>("start_recording", { pid, includeMic }),
  /** `title` = meeting subject, used as the transcript file name */
  stop: (title: string | null) => invoke<string | null>("stop_recording", { title }),
  pause: (paused: boolean) => invoke<void>("pause_recording", { paused }),
  retrySetup: () => invoke<void>("retry_setup"),
  setNotchSize: (width: number, height: number) =>
    invoke<void>("set_notch_size", { width, height }),
  openTranscript: () => invoke<void>("open_transcript"),
  openFolder: () => invoke<void>("open_transcripts_folder"),
  quit: () => invoke<void>("quit_app"),
  claudeStatus: () => invoke<ClaudeStatus>("claude_status"),
  /** re-checks Claude's sign-in (throttled in the backend) */
  claudeRefresh: () => invoke<void>("claude_refresh"),
  /** `window` = show the CLI's own sign-in console instead of a background sign-in */
  claudeLogin: (window: boolean) => invoke<void>("claude_login", { window }),
  claudeInstall: () => invoke<void>("claude_install"),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** downloads the newer build, swaps the exe and restarts — only on the user's click */
  updateInstall: () => invoke<void>("update_install"),
};

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}
