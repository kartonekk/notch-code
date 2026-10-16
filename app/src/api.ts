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
}

export interface GeminiModel {
  id: string;
  label: string;
  description?: string | null;
}

/** Gemini's sign-in: Antigravity CLI with a Google account (the Gemini subscription) */
export interface GeminiStatus {
  state: ClaudeAuthState;
  message: string | null;
  /** the Google account it is signed in with */
  account: string | null;
  models: GeminiModel[];
  /** how much of the plan is used, from Antigravity's `/usage` (while signed in) */
  usage: GeminiUsage | null;
}

export interface GeminiUsage {
  quotas: GeminiQuota[];
  /** unix ms when it was read */
  at: number;
}

/** one of the plan's limits; models in a group share it */
export interface GeminiQuota {
  /** "Gemini Models", "Claude and GPT models" */
  group: string;
  /** "Gemini Flash, Gemini Pro" */
  models: string | null;
  /** "Weekly Limit Remaining" */
  name: string;
  /** "weekly", "daily", … */
  window: string | null;
  /** 0..1 */
  used: number;
  /** unix ms when it refills */
  resets_at: number | null;
}

/** the agent a session uses */
export type Provider = "claude" | "gemini";

export const PROVIDER_NAME: Record<Provider, string> = { claude: "Claude", gemini: "Gemini" };

/** what's playing on this computer (Windows' media controls) */
export interface Media {
  title: string;
  artist: string | null;
  /** the app playing it ("Spotify", "Chrome", …) */
  app: string;
  playing: boolean;
  /** where it was at `updated_at` (unix ms) */
  position_ms: number | null;
  duration_ms: number | null;
  updated_at: number | null;
  can_prev: boolean;
  can_next: boolean;
  /** album art as a data URL */
  thumb: string | null;
}

export interface Settings {
  autostart: boolean;
  /** transcription runs on the graphics card (else the CPU) */
  gpu: boolean;
  /** there is a graphics card the transcription can use */
  gpu_available: boolean;
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

/** One Claude conversation in one folder; several can work at once. */
export interface SessionStatus {
  id: number;
  /** folder Claude works in */
  project: string;
  /** the agent this session uses */
  provider: Provider;
  running: boolean;
  /** a conversation is going on (the next message continues it) */
  has_session: boolean;
  /** the step in progress, for the collapsed bar */
  current: string | null;
  /** unix ms when that step started (null while Claude is thinking) */
  current_since: number | null;
  /** unix ms when the running message was sent */
  started_at: number | null;
  /** model of the last run, as Claude reported it */
  model: string | null;
  /** caveman level Claude answers with ("ultra", …); null = normal answers */
  caveman: string | null;
  /** tokens Claude has written for the running (or last) message, live; 0 = none reported */
  tokens: number;
}

export interface AgentStatus {
  sessions: SessionStatus[];
  /** recently used folders, newest first */
  recent: string[];
  /** the 5-hour usage window, as last reported by Claude */
  limit: Limit | null;
  /** how much of the plan is used, from Claude Code's `/usage` */
  usage: Usage | null;
  /** the agent new sessions use */
  default_provider: Provider;
}

export interface Usage {
  /** current 5-hour session, 0..1 */
  session: number;
  /** current week (all models), 0..1 */
  week: number | null;
  /** the whole `/usage` report, as Claude Code wrote it */
  report: string;
  /** unix ms when it was read */
  at: number;
}

export interface Limit {
  status: "allowed" | "allowed_warning" | "rejected";
  /** 0..1, when Claude reports it */
  used: number | null;
  /** unix ms when the window resets */
  resets_at: number | null;
}

export type StepIcon = "read" | "create" | "edit" | "run" | "search" | "web" | "plan" | "agent" | "other";

/** what "/" can run in a session's folder */
export interface SlashCommand {
  /** without the slash; plugin ones as "plugin:name" */
  name: string;
  description: string | null;
  /** what goes after the name, e.g. "[--share|--all]" */
  hint: string | null;
  /** "project" | "user" | "built-in" | the plugin's name | "claude" */
  source: string;
}

/** something the permission mode didn't allow; `rule` unlocks it */
export interface Denied {
  label: string;
  rule: string;
}

/** `sid` = the session it belongs to */
export type AgentEvent = { sid: number } & (
  | { kind: "start"; run: number; prompt: string }
  /** the skills Claude was given when it started */
  | { kind: "init"; run: number; skills: string[] }
  /** written while Claude worked, passed on to it right away */
  | { kind: "followup"; run: number; prompt: string }
  | { kind: "text"; run: number; text: string }
  /** `at` = unix ms when the step started */
  | { kind: "step"; run: number; id: string; icon: StepIcon; label: string; detail: string | null; at: number }
  | { kind: "step_done"; run: number; id: string; ok: boolean; output: string | null; label: string; duration_ms: number }
  | { kind: "done"; run: number; ok: boolean; stopped: boolean; message: string | null; duration_ms: number; denied: Denied[]; tokens: number }
);

export interface FileChange {
  path: string;
  name: string;
  status: "modified" | "created" | "deleted";
  added: number;
  removed: number;
}

export interface ReviewStatus {
  /** project folder -> Claude's changes there, waiting for accept / reject */
  projects: Record<string, FileChange[]>;
  /** live diff: changed files open in VS Code as they change */
  live: boolean;
  /** VS Code was found */
  vscode: boolean;
}

/**
 * Spoken prompts. off: microphone closed · listening: waiting for "Claude" ·
 * armed: waiting for the prompt · hearing: taking it (`text` = heard so far)
 */
export interface VoiceStatus {
  /** the wake word is on */
  wake: boolean;
  phase: "off" | "listening" | "armed" | "hearing";
  text: string | null;
  error: string | null;
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
  /** the open notch takes the keyboard (`true`) or hands it back to the previous window */
  notchFocus: (on: boolean) => invoke<void>("notch_focus", { on }),
  openTranscript: () => invoke<void>("open_transcript"),
  openFolder: () => invoke<void>("open_transcripts_folder"),
  quit: () => invoke<void>("quit_app"),
  claudeStatus: () => invoke<ClaudeStatus>("claude_status"),
  /** re-checks Claude's sign-in (throttled in the backend) */
  claudeRefresh: () => invoke<void>("claude_refresh"),
  /** opens Claude's sign-in console (it sends you to the browser) */
  claudeLogin: () => invoke<void>("claude_login"),
  claudeLogout: () => invoke<void>("claude_logout"),
  /** re-checks Claude's sign-in now */
  claudeRecheck: () => invoke<void>("claude_recheck"),
  claudeInstall: () => invoke<void>("claude_install"),
  geminiStatus: () => invoke<GeminiStatus>("gemini_status"),
  geminiRefresh: () => invoke<void>("gemini_refresh"),
  /** opens Antigravity CLI to sign in with Google (installs it first if needed) */
  geminiLogin: () => invoke<void>("gemini_login"),
  geminiLogout: () => invoke<void>("gemini_logout"),
  geminiInstall: () => invoke<void>("gemini_install"),
  geminiModels: () => invoke<GeminiModel[]>("gemini_models"),
  mediaStatus: () => invoke<Media | null>("media_status"),
  mediaControl: (action: "toggle" | "next" | "prev") => invoke<void>("media_control", { action }),
  settingsGet: () => invoke<Settings>("settings_get"),
  setAutostart: (on: boolean) => invoke<void>("set_autostart", { on }),
  /** saves the CPU / GPU choice and reloads the speech model on it */
  setGpu: (on: boolean) => invoke<void>("set_gpu", { on }),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** downloads the newer build, swaps the exe and restarts — only on the user's click */
  updateInstall: () => invoke<void>("update_install"),
  agentStatus: () => invoke<AgentStatus>("agent_status"),
  /** reads the plan usage again (it arrives with agent-status / gemini) */
  agentRefreshUsage: (provider: Provider) => invoke<void>("agent_refresh_usage", { provider }),
  /** new session in `path` (with `provider`, else the default agent); returns its id */
  agentOpen: (path: string, provider: Provider | null = null) => invoke<number>("agent_open", { path, provider }),
  /** switches an idle session to the other agent (new conversation) */
  agentSetProvider: (sid: number, provider: Provider) => invoke<void>("agent_set_provider", { sid, provider }),
  /** the agent new sessions use */
  agentSetDefaultProvider: (provider: Provider) => invoke<void>("agent_set_default_provider", { provider }),
  agentClose: (sid: number) => invoke<void>("agent_close", { sid }),
  /** `allow` = extra --allowedTools rules (after the user allowed a denied step) */
  /** `model` = alias ("opus", "sonnet", …) or full name; null = Claude Code's default */
  agentSend: (sid: number, prompt: string, mode: AgentMode, allow: string[] = [], model: string | null = null) =>
    invoke<void>("agent_send", { sid, prompt, mode, allow, model }),
  /** hands a message to Claude while it works; fails when it isn't taking any (then send it next) */
  agentInject: (sid: number, prompt: string) => invoke<void>("agent_inject", { sid, prompt }),
  /** switches every session Claude is working in to `mode` right away */
  agentSetMode: (mode: AgentMode) => invoke<void>("agent_set_mode", { mode }),
  agentStop: (sid: number) => invoke<void>("agent_stop", { sid }),
  /** forget the conversation; the next message starts a new one */
  agentNew: (sid: number) => invoke<void>("agent_new", { sid }),
  /** slash commands for the chat bar's "/" menu */
  agentCommands: (sid: number) => invoke<SlashCommand[]>("agent_commands", { sid }),
  /** move an idle session to another folder */
  agentSetProject: (sid: number, path: string) => invoke<void>("agent_set_project", { sid, path }),
  /** system folder picker */
  agentPickFolder: (current: string | null) => invoke<string | null>("agent_pick_folder", { current }),
  agentOpenFolder: (path: string) => invoke<void>("agent_open_folder", { path }),
  /** takes a folder off the recent list */
  agentForget: (path: string) => invoke<void>("agent_forget", { path }),
  /** a cmd window in that folder */
  openTerminal: (path: string) => invoke<void>("open_terminal", { path }),
  /** saves a pasted image for Claude; returns its path */
  agentSaveImage: (bytes: Uint8Array, ext: string) => invoke<string>("agent_save_image", bytes, { headers: { "x-ext": ext } }),
  /** system file picker for images */
  agentPickImages: () => invoke<string[]>("agent_pick_images"),
  reviewStatus: () => invoke<ReviewStatus>("review_status"),
  /** keep Claude's changes (one file, or all when `path` is omitted) */
  reviewAccept: (project: string, path?: string) => invoke<void>("review_accept", { project, path: path ?? null }),
  /** restore the originals (one file, or all when `path` is omitted) */
  reviewReject: (project: string, path?: string) => invoke<void>("review_reject", { project, path: path ?? null }),
  /** original ↔ current in VS Code */
  reviewDiff: (project: string, path: string) => invoke<void>("review_diff", { project, path }),
  reviewSetLive: (live: boolean) => invoke<void>("review_set_live", { live }),
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  /** wake word "Claude" on/off (keeps the microphone open while on) */
  voiceSetWake: (on: boolean) => invoke<void>("voice_set_wake", { on }),
  /** take the next thing said as a prompt; again to cancel */
  voiceTalk: () => invoke<void>("voice_talk"),
};

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}
