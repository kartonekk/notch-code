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
};

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}
