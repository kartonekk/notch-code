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

/** "reckless" = no permission checks, and changes are kept without review */
export type AgentMode = "auto" | "plan" | "acceptEdits" | "reckless";

export interface AgentStatus {
  running: boolean;
  /** folder Claude works in */
  project: string | null;
  /** recently used folders, newest first */
  recent: string[];
  /** a conversation is going on (the next message continues it) */
  has_session: boolean;
  /** the step in progress, for the collapsed bar */
  current: string | null;
  /** unix ms when the running message was sent */
  started_at: number | null;
  /** the 5-hour usage window, as last reported by Claude */
  limit: Limit | null;
  /** model of the last run, as Claude reported it */
  model: string | null;
}

export interface Limit {
  status: "allowed" | "allowed_warning" | "rejected";
  /** 0..1, when Claude reports it */
  used: number | null;
  /** unix ms when the window resets */
  resets_at: number | null;
}

export type StepIcon = "read" | "create" | "edit" | "run" | "search" | "web" | "plan" | "agent" | "other";

/** something the permission mode didn't allow; `rule` unlocks it */
export interface Denied {
  label: string;
  rule: string;
}

export type AgentEvent =
  | { kind: "start"; run: number; prompt: string }
  | { kind: "text"; run: number; text: string }
  | { kind: "step"; run: number; id: string; icon: StepIcon; label: string; detail: string | null }
  | { kind: "step_done"; run: number; id: string; ok: boolean; output: string | null }
  | { kind: "done"; run: number; ok: boolean; stopped: boolean; message: string | null; duration_ms: number; denied: Denied[] };

export interface FileChange {
  path: string;
  name: string;
  status: "modified" | "created" | "deleted";
  added: number;
  removed: number;
}

export interface ReviewStatus {
  /** Claude's changes waiting for accept / reject */
  files: FileChange[];
  /** live diff: changed files open in VS Code as they change */
  live: boolean;
  /** VS Code was found */
  vscode: boolean;
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
  agentStatus: () => invoke<AgentStatus>("agent_status"),
  /** `allow` = extra --allowedTools rules (after the user allowed a denied step) */
  /** `model` = alias ("opus", "sonnet", …) or full name; null = Claude Code's default */
  agentSend: (prompt: string, mode: AgentMode, allow: string[] = [], model: string | null = null) =>
    invoke<void>("agent_send", { prompt, mode, allow, model }),
  agentStop: () => invoke<void>("agent_stop"),
  /** forget the conversation; the next message starts a new one */
  agentNew: () => invoke<void>("agent_new"),
  agentSetProject: (path: string) => invoke<void>("agent_set_project", { path }),
  /** system folder picker; the chosen folder becomes the project */
  agentPickProject: () => invoke<string | null>("agent_pick_project"),
  agentOpenProject: () => invoke<void>("agent_open_project"),
  reviewStatus: () => invoke<ReviewStatus>("review_status"),
  /** keep Claude's changes (one file, or all when `path` is omitted) */
  reviewAccept: (path?: string) => invoke<void>("review_accept", { path: path ?? null }),
  /** restore the originals (one file, or all when `path` is omitted) */
  reviewReject: (path?: string) => invoke<void>("review_reject", { path: path ?? null }),
  /** original ↔ current in VS Code */
  reviewDiff: (path: string) => invoke<void>("review_diff", { path }),
  reviewSetLive: (live: boolean) => invoke<void>("review_set_live", { live }),
};

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}
