import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ClipboardEvent as ReactClipboardEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { AgentEvent, AgentMode, AgentStatus, api, Denied, Limit, on, Provider, PROVIDER_NAME, SessionStatus, SlashCommand, StepIcon } from "./api";
import { Markdown } from "./Markdown";
import { parseUsageReport, UsageCard } from "./Usage";
import { MicButton, Voice, VoiceStrip } from "./Voice";
import {
  AlertIcon,
  basename,
  CheckIcon,
  ChevronIcon,
  CopyIcon,
  CrossIcon,
  formatDuration,
  ImageIcon,
  formatSpan,
  RetryIcon,
  SendIcon,
  StepGlyph,
  store,
} from "./ui";

// ---- state ----------------------------------------------------------------------

export type FeedItem =
  | { type: "prompt"; run: number; text: string }
  /** written while Claude worked; it got it at its next step */
  | { type: "followup"; run: number; text: string }
  /** the skills Claude started with (shown when they differ from the last run's) */
  | { type: "skills"; run: number; skills: string[] }
  | { type: "text"; run: number; text: string }
  | {
      type: "step";
      run: number;
      id: string;
      icon: StepIcon;
      label: string;
      detail: string | null;
      state: "running" | "ok" | "error";
      output?: string | null;
      /** unix ms */
      started: number;
      duration_ms?: number;
    }
  | { type: "done"; run: number; ok: boolean; stopped: boolean; message: string | null; duration_ms: number; denied: Denied[]; steps: number };

/** What the collapsed notch says about a run that ended while it was closed. */
export interface AgentNotice {
  tone: "ok" | "warn" | "error";
  text: string;
}

const MAX_ITEMS = 2000;
/** a step that took at least this long says so in the notch when it finishes (builds etc.) */
const LONG_STEP_MS = 15_000;
const FLASH_MS = 8_000;

function applyEvent(prev: FeedItem[], e: AgentEvent): FeedItem[] {
  let next: FeedItem[];
  switch (e.kind) {
    case "start":
      next = [...prev, { type: "prompt", run: e.run, text: e.prompt }];
      break;
    case "followup":
      next = [...prev, { type: "followup", run: e.run, text: e.prompt }];
      break;
    case "init": {
      // Claude lists them at every message; say it once, and again only when the skills change
      const last = [...prev].reverse().find((i) => i.type === "skills");
      if (last?.type === "skills" && last.skills.join("\n") === e.skills.join("\n")) return prev;
      next = [...prev, { type: "skills", run: e.run, skills: e.skills }];
      break;
    }
    case "text":
      next = [...prev, { type: "text", run: e.run, text: e.text }];
      break;
    case "step":
      next = [...prev, { type: "step", run: e.run, id: e.id, icon: e.icon, label: e.label, detail: e.detail, state: "running", started: e.at }];
      break;
    case "step_done":
      next = prev.map((i) =>
        i.type === "step" && i.id === e.id ? { ...i, state: e.ok ? "ok" : "error", output: e.output, duration_ms: e.duration_ms } : i,
      );
      break;
    case "done": {
      const steps = prev.filter((i) => i.type === "step" && i.run === e.run).length;
      next = [
        ...prev.map((i) => (i.type === "step" && i.run === e.run && i.state === "running" ? { ...i, state: e.ok ? ("ok" as const) : ("error" as const) } : i)),
        { type: "done", run: e.run, ok: e.ok, stopped: e.stopped, message: e.message, duration_ms: e.duration_ms, denied: e.denied, steps },
      ];
      break;
    }
    default:
      return prev;
  }
  return next.length > MAX_ITEMS ? next.slice(-MAX_ITEMS) : next;
}

function noticeFor(e: Extract<AgentEvent, { kind: "done" }>, project: string): AgentNotice | null {
  if (e.stopped) return null;
  const name = basename(project);
  if (e.denied.length > 0) return { tone: "warn", text: `${name}: Claude needs your OK to continue` };
  if (!e.ok) return { tone: "error", text: `${name}: ${e.message ?? "Claude hit a problem"}` };
  return { tone: "ok", text: `${name} · done in ${formatSpan(e.duration_ms)}` };
}

/** an image going with the next message; `preview` for pasted ones */
interface Attachment {
  path: string;
  preview?: string;
}

const isImagePath = (p: string) => /\.(png|jpe?g|gif|webp)$/i.test(p);

const IMAGES_HEADER = "\n\nAttached images (open them with the Read tool):\n";

/** Claude Code reads images from disk, so the message lists their paths. */
function withImages(text: string, paths: string[]) {
  if (paths.length === 0) return text;
  return `${text.trim() || "Look at the attached image."}${IMAGES_HEADER}${paths.map((p) => `- ${p}`).join("\n")}`;
}

/** the other way round, for showing a sent message */
function splitImages(prompt: string): { text: string; images: string[] } {
  const at = prompt.indexOf(IMAGES_HEADER);
  if (at < 0) return { text: prompt, images: [] };
  const images = prompt
    .slice(at + IMAGES_HEADER.length)
    .split("\n")
    .map((l) => l.replace(/^- /, "").trim())
    .filter(Boolean);
  return { text: prompt.slice(0, at), images };
}

/** "01:23" for the step in progress once it has run a few seconds; "" otherwise */
export function stepTime(s: SessionStatus, now: number) {
  return s.running && s.current_since && now - s.current_since >= 3000 ? formatDuration(now - s.current_since) : "";
}

/** "Auto" is the no-brakes mode: nothing is asked, every change is kept. */
const MODES: { id: AgentMode; label: string; icon: StepIcon | "flame"; hint: string; title: string }[] = [
  { id: "reckless", label: "Auto", icon: "flame", hint: "No brakes · changes kept automatically", title: "Claude runs anything it wants and every change is kept (Shift+Tab)" },
  { id: "acceptEdits", label: "Edits", icon: "edit", hint: "Commands need your OK · review below the notch", title: "Claude edits files; commands need your OK; changes wait for review (Shift+Tab)" },
  { id: "plan", label: "Plan", icon: "plan", hint: "Read-only, proposes a plan", title: "Read-only: Claude looks around and proposes a plan (Shift+Tab)" },
];

export const MODELS: { id: string; label: string; title: string }[] = [
  { id: "", label: "Default", title: "Whatever Claude Code uses by default" },
  { id: "fable", label: "Fable", title: "Latest Fable" },
  { id: "opus", label: "Opus", title: "Latest Opus: most capable" },
  { id: "sonnet", label: "Sonnet", title: "Latest Sonnet: fast and capable" },
  { id: "haiku", label: "Haiku", title: "Latest Haiku: fastest, lightest on the limit" },
];

/** handled by the app itself, not sent to Claude */
const LOCAL_COMMANDS: SlashCommand[] = [
  { name: "clear", description: "Start a new conversation (Ctrl+N)", hint: null, source: "notch" },
  { name: "model", description: `Switch the model: ${MODELS.map((m) => m.label.toLowerCase()).join(", ")}`, hint: "[model]", source: "notch" },
];

/** `c`'s name (or the part after "plugin:") starts with `query` */
function namedLike(c: SlashCommand, query: string) {
  const name = c.name.toLowerCase();
  const q = query.toLowerCase();
  return name.startsWith(q) || name.slice(name.indexOf(":") + 1).startsWith(q);
}

/** Commands named like `query` first, then those containing it. */
function matchCommands(all: SlashCommand[], query: string): SlashCommand[] {
  const q = query.toLowerCase();
  const rank = (c: SlashCommand) => {
    if (namedLike(c, q)) return 0;
    return c.name.toLowerCase().includes(q) || (c.description ?? "").toLowerCase().includes(q) ? 1 : 2;
  };
  return all
    .map((c) => ({ c, r: rank(c) }))
    .filter((x) => x.r < 2)
    .sort((a, b) => a.r - b.r)
    .map((x) => x.c);
}

/** "/review 12": Claude Code only runs it at the start of a message, never as a follow-up mid-run */
const isCommand = (text: string) => /^\/\S/.test(text.trim());

/** What goes out next from a queue: a command on its own, else the texts up to the next command (as one message). */
function nextBatch(queued: string[]): [string[], string[]] {
  const end = isCommand(queued[0] ?? "") ? 1 : queued.findIndex(isCommand);
  const n = end < 0 ? queued.length : end;
  return [queued.slice(0, n), queued.slice(n)];
}

function cachedLimit(): Limit | null {
  try {
    const l = JSON.parse(store.get("agent.limit") ?? "null") as Limit | null;
    return l && (l.resets_at ?? 0) > Date.now() ? l : null;
  } catch {
    return null;
  }
}

type PerSession<T> = Record<number, T>;

/** Every Claude session: their feeds, queued messages and finished notices. */
export function useAgents(expanded: boolean, viewing: number | null) {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [feeds, setFeeds] = useState<PerSession<FeedItem[]>>({});
  const [queues, setQueues] = useState<PerSession<string[]>>({});
  const [notices, setNotices] = useState<PerSession<AgentNotice>>({});
  /** finished while you weren't looking at it */
  const [unseen, setUnseen] = useState<PerSession<AgentNotice["tone"]>>({});
  /** messages handed to Claude while it worked, in the run going on now */
  const followups = useRef<PerSession<string[]>>({});
  /** what you wrote for a run you then stopped: back into the chat bar */
  const [restored, setRestored] = useState<PerSession<string[]>>({});
  const [mode, setModeState] = useState<AgentMode>(() => {
    const m = store.get("agent.mode");
    return m === "plan" || m === "acceptEdits" ? m : "reckless";
  });
  const [model, setModelState] = useState(() => store.get("agent.model") ?? "");
  const [error, setError] = useState<string | null>(null);
  /** a short-lived line for the collapsed notch: a long step finished, a spoken prompt went out */
  const [flash, setFlashState] = useState<AgentNotice | null>(null);
  const flashTimer = useRef<number | null>(null);
  const showFlash = (n: AgentNotice) => {
    if (flashTimer.current) window.clearTimeout(flashTimer.current);
    setFlashState(n);
    flashTimer.current = window.setTimeout(() => setFlashState(null), FLASH_MS);
  };
  const flashRef = useRef(showFlash);
  flashRef.current = showFlash;

  // the event listener is registered once; it reads current values through this
  const live = useRef({ expanded, viewing, mode, model, queues, status });
  live.current = { expanded, viewing, mode, model, queues, status };

  const call = (p: Promise<unknown>) => {
    setError(null);
    p.catch((e) => setError(String(e)));
  };

  useEffect(() => {
    api.agentStatus().then(setStatus).catch(() => {});
    const subs = [
      on<AgentStatus>("agent-status", (s) => {
        live.current.status = s;
        setStatus(s);
      }),
      on<AgentEvent>("agent", (e) => {
        setFeeds((prev) => ({ ...prev, [e.sid]: applyEvent(prev[e.sid] ?? [], e) }));
        const { expanded, viewing, mode, model, queues, status } = live.current;
        const project = status?.sessions.find((s) => s.id === e.sid)?.project ?? "";
        if (e.kind === "step_done" && e.duration_ms >= LONG_STEP_MS) {
          flashRef.current({
            tone: e.ok ? "ok" : "error",
            text: `${basename(project)}: ${e.label} — ${e.ok ? "finished" : "failed"} after ${formatSpan(e.duration_ms)}`,
          });
        }
        if (e.kind === "followup") {
          followups.current = { ...followups.current, [e.sid]: [...(followups.current[e.sid] ?? []), e.prompt] };
        }
        if (e.kind !== "done") return;
        const notice = noticeFor(e, project);
        if (notice && !expanded) setNotices((prev) => ({ ...prev, [e.sid]: notice }));
        if (notice && (!expanded || viewing !== e.sid)) setUnseen((prev) => ({ ...prev, [e.sid]: notice.tone }));
        const handed = followups.current[e.sid] ?? [];
        followups.current = { ...followups.current, [e.sid]: [] };
        // what couldn't reach Claude in time goes out now, as one message; a command
        // goes alone, and whatever follows it waits for that run to finish
        const queued = queues[e.sid] ?? [];
        const [next, rest] = e.stopped ? [[], []] : nextBatch(queued);
        if (queued.length > 0) setQueues((prev) => ({ ...prev, [e.sid]: rest }));
        if (e.stopped) {
          // stopped: everything written for this run goes back into the chat bar
          const back = [...handed, ...queued];
          if (back.length > 0) setRestored((prev) => ({ ...prev, [e.sid]: [...(prev[e.sid] ?? []), ...back] }));
        } else if (next.length > 0) {
          call(api.agentSend(e.sid, next.join("\n\n"), mode, [], model || null));
        }
      }),
    ];
    return () => subs.forEach((p) => p.then((un) => un()));
  }, []);

  // remember the last known 5-hour window across restarts
  const reported = status?.limit ?? null;
  useEffect(() => {
    if (reported) store.set("agent.limit", JSON.stringify(reported));
  }, [reported]);
  const limit = reported ?? cachedLimit();

  // opening the notch shows the results: the "finished" look has done its job
  useEffect(() => {
    if (expanded) setNotices({});
  }, [expanded]);
  useEffect(() => {
    if (expanded && viewing != null) setUnseen(({ [viewing]: _, ...rest }) => rest);
  }, [expanded, viewing]);

  const sessions = status?.sessions ?? [];
  const session = (sid: number) => sessions.find((s) => s.id === sid) ?? null;
  const drop = <T,>(m: PerSession<T>, sid: number) => {
    const { [sid]: _, ...rest } = m;
    return rest;
  };

  return {
    status,
    sessions,
    recent: status?.recent ?? [],
    session,
    feed: (sid: number) => feeds[sid] ?? [],
    queue: (sid: number) => queues[sid] ?? [],
    notices,
    unseen,
    running: sessions.filter((s) => s.running),
    mode,
    model,
    limit,
    /** how much of the plan is used (Claude Code's `/usage`) */
    usage: status?.usage ?? null,
    error,
    flash,
    showFlash,
    setMode: (m: AgentMode) => {
      setModeState(m);
      store.set("agent.mode", m);
      // Claude switches too if it's working
      call(api.agentSetMode(m));
    },
    setModel: (m: string) => {
      setModelState(m);
      store.set("agent.model", m);
    },
    /** sends now; while Claude works it gets it at its next step (or right after, if it's already answering; a /command always waits) */
    send: (sid: number, prompt: string, allow: string[] = []) => {
      const text = prompt.trim();
      if (!text) return;
      const busy = (id: number) => !!live.current.status?.sessions.find((s) => s.id === id)?.running;
      if (!busy(sid)) {
        call(api.agentSend(sid, text, mode, allow, model || null));
        return;
      }
      if (isCommand(text)) {
        setQueues((prev) => ({ ...prev, [sid]: [...(prev[sid] ?? []), text] }));
        return;
      }
      api.agentInject(sid, text).catch(() => {
        // Claude isn't taking messages anymore: next in line, or now if it has already finished
        if (busy(sid)) setQueues((prev) => ({ ...prev, [sid]: [...(prev[sid] ?? []), text] }));
        else call(api.agentSend(sid, text, live.current.mode, allow, live.current.model || null));
      });
    },
    /** messages to put back into the chat bar (after a stop); taking them clears them */
    restored: (sid: number) => restored[sid] ?? [],
    takeRestored: (sid: number) => setRestored((prev) => drop(prev, sid)),
    unqueue: (sid: number, index: number) => setQueues((prev) => ({ ...prev, [sid]: (prev[sid] ?? []).filter((_, i) => i !== index) })),
    stop: (sid: number) => call(api.agentStop(sid)),
    newChat: (sid: number) => {
      setFeeds((prev) => ({ ...prev, [sid]: [] }));
      call(api.agentNew(sid));
    },
    /** the agent new sessions use */
    defaultProvider: (status?.default_provider ?? "claude") as Provider,
    setDefaultProvider: (p: Provider) => call(api.agentSetDefaultProvider(p)),
    /** switches an idle session to the other agent (new conversation) */
    setProvider: (sid: number, p: Provider) => {
      setFeeds((prev) => ({ ...prev, [sid]: [] }));
      call(api.agentSetProvider(sid, p));
    },
    /** opens a session in `path` (with `provider`, else the default agent) and returns its id */
    open: async (path: string, provider?: Provider) => {
      setError(null);
      try {
        return await api.agentOpen(path, provider ?? null);
      } catch (e) {
        setError(String(e));
        return null;
      }
    },
    close: (sid: number) => {
      setFeeds((prev) => drop(prev, sid));
      setQueues((prev) => drop(prev, sid));
      setNotices((prev) => drop(prev, sid));
      setUnseen((prev) => drop(prev, sid));
      setRestored((prev) => drop(prev, sid));
      call(api.agentClose(sid));
    },
    forget: (path: string) => call(api.agentForget(path)),
    moveTo: (sid: number, path: string) => {
      setFeeds((prev) => ({ ...prev, [sid]: [] }));
      call(api.agentSetProject(sid, path));
    },
    pickFolder: async (current: string | null) => {
      setError(null);
      try {
        return await api.agentPickFolder(current);
      } catch (e) {
        setError(String(e));
        return null;
      }
    },
  };
}

export type Agents = ReturnType<typeof useAgents>;

// ---- tab ------------------------------------------------------------------------
//
// One session, top to bottom:
//   feed            older runs fold into one line, the latest is open
//   queue           what you wrote while Claude worked (sent when it finishes)
//   run strip       what Claude is doing right now (Esc stops)
//   composer        message + mode + model; Enter sends or queues

const SUGGESTIONS = [
  "Explain how this project works",
  "Find bugs and fix them",
  "Run the tests and fix what fails",
  "Review my uncommitted changes",
];

const CYCLE: AgentMode[] = MODES.map((m) => m.id);

export function CodeTab({ agents, session, voice, onHold }: { agents: Agents; session: SessionStatus; voice: Voice; onHold: (hold: boolean) => void }) {
  const sid = session.id;
  const items = agents.feed(sid);
  const queue = agents.queue(sid);
  const running = session.running;
  const gemini = session.provider === "gemini";
  const agentName = PROVIDER_NAME[session.provider];
  const draftKey = `agent.draft.${session.project}`;
  const [draft, setDraft] = useState(() => store.get(draftKey) ?? "");
  const [modelOpen, setModelOpen] = useState(false);
  const [focused, setFocused] = useState(false);
  const [scrolledUp, setScrolledUp] = useState(false);
  const feedRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const stick = useRef(true);
  const reckless = agents.mode === "reckless";
  const [images, setImages] = useState<Attachment[]>([]);
  const [pickingImages, setPickingImages] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [imageError, setImageError] = useState<string | null>(null);
  // "/" menu: shown while the draft is just "/name"
  const [commands, setCommands] = useState<SlashCommand[]>([]);
  const [slashIndex, setSlashIndex] = useState(0);
  /** picked with the arrows, not just the first match */
  const [slashMoved, setSlashMoved] = useState(false);
  const [slashDismissed, setSlashDismissed] = useState(false);
  const slashQuery = /^\/(\S*)$/.exec(draft)?.[1] ?? null;
  const slashOn = slashQuery != null && focused && !slashDismissed;
  const slashMatches = useMemo(() => {
    if (slashQuery == null || !slashOn) return [];
    const local = new Set(LOCAL_COMMANDS.map((c) => c.name));
    return matchCommands([...LOCAL_COMMANDS, ...commands.filter((c) => !local.has(c.name))], slashQuery);
  }, [slashOn, slashQuery, commands]);
  useEffect(() => {
    if (slashOn) api.agentCommands(sid).then(setCommands).catch(() => setCommands([]));
  }, [slashOn, sid]);
  useEffect(() => {
    setSlashIndex(0);
    setSlashMoved(false);
  }, [slashQuery]);
  const typingCommand = slashQuery != null;
  useEffect(() => {
    if (!typingCommand) setSlashDismissed(false);
  }, [typingCommand]);

  // keep the notch open while typing, choosing a model or picking images
  const hold = modelOpen || focused || pickingImages;
  useEffect(() => onHold(hold), [hold, onHold]);
  useEffect(() => () => onHold(false), [onHold]);

  useEffect(() => store.set(draftKey, draft), [draft, draftKey]);

  // stopped Claude: what you wrote for that run comes back, ready to edit and send again
  const restored = agents.restored(sid);
  useEffect(() => {
    if (restored.length === 0) return;
    setDraft((prev) => [...restored, prev.trim()].filter(Boolean).join("\n\n"));
    agents.takeRestored(sid);
    inputRef.current?.focus();
  }, [restored, sid]);

  // autosize the composer (1–5 lines)
  useLayoutEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 110)}px`;
  }, [draft]);

  // newest at the bottom; scrolling up pauses following
  useLayoutEffect(() => {
    const el = feedRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [items, running, queue.length]);
  useLayoutEffect(() => {
    stick.current = true;
    setScrolledUp(false);
    const el = feedRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, []);

  // elapsed time of the running message
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [running]);

  const lastPrompt = useMemo(() => {
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i];
      if (it.type === "prompt") return it.text;
    }
    return null;
  }, [items]);

  const send = (text: string) => {
    if (!text.trim()) return;
    stick.current = true;
    agents.send(sid, text);
  };
  /** /clear and /model are the app's own; true when `text` was one of them */
  const runLocal = (text: string) => {
    const m = /^\/(\S+)\s*([\s\S]*)$/.exec(text.trim());
    if (!m) return false;
    const [, name, arg] = m;
    if (name === "clear" || name === "new") {
      if (!running) agents.newChat(sid);
    } else if (name === "model") {
      const want = arg.trim().toLowerCase();
      const known = MODELS.find((x) => x.id === want || x.label.toLowerCase() === want);
      if (!want) setModelOpen(true);
      else agents.setModel(known ? known.id : arg.trim());
    } else return false;
    return true;
  };
  const submit = () => {
    if (!draft.trim() && images.length === 0) return;
    if (images.length === 0 && runLocal(draft)) {
      setDraft("");
      return;
    }
    send(withImages(draft, images.map((i) => i.path)));
    setDraft("");
    images.forEach((i) => i.preview && URL.revokeObjectURL(i.preview));
    setImages([]);
  };

  // ---- images: paste, drop onto the panel, or pick
  const addPaths = (paths: string[]) => {
    const ok = paths.filter(isImagePath);
    if (ok.length < paths.length) setImageError("Only images (PNG, JPG, GIF, WebP) can be attached.");
    if (ok.length) setImages((prev) => [...prev, ...ok.filter((p) => !prev.some((i) => i.path === p)).map((path) => ({ path }))]);
  };
  const onPaste = (e: ReactClipboardEvent) => {
    const files = Array.from(e.clipboardData.files).filter((f) => f.type.startsWith("image/"));
    if (files.length === 0) return;
    e.preventDefault();
    setImageError(null);
    for (const file of files) {
      const ext = file.type.split("/")[1]?.replace("jpeg", "jpg") ?? "png";
      file
        .arrayBuffer()
        .then((buf) => api.agentSaveImage(new Uint8Array(buf), ext))
        .then((path) => setImages((prev) => [...prev, { path, preview: URL.createObjectURL(file) }]))
        .catch((err) => setImageError(String(err)));
    }
  };
  /** Enter runs a command that takes nothing; otherwise (and on Tab) it goes into the chat bar for its arguments */
  const pickCommand = (c: SlashCommand, run: boolean) => {
    if (run && !c.hint && images.length === 0) {
      if (!runLocal(`/${c.name}`)) send(`/${c.name}`);
      setDraft("");
    } else setDraft(`/${c.name} `);
    inputRef.current?.focus();
  };

  const pickImages = async () => {
    setImageError(null);
    setPickingImages(true);
    try {
      addPaths(await api.agentPickImages());
    } catch (err) {
      setImageError(String(err));
    } finally {
      setPickingImages(false);
      inputRef.current?.focus();
    }
  };
  const addPathsRef = useRef(addPaths);
  addPathsRef.current = addPaths;
  useEffect(() => {
    const sub = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setDropping(true);
      else if (p.type === "leave") setDropping(false);
      else if (p.type === "drop") {
        setDropping(false);
        setImageError(null);
        addPathsRef.current(p.paths);
      }
    });
    return () => {
      sub.then((un) => un());
    };
  }, []);

  // shortcuts for this session
  const keys = useRef<(e: KeyboardEvent) => boolean>(() => false);
  keys.current = (e) => {
    if (e.shiftKey && e.key === "Tab") agents.setMode(CYCLE[(CYCLE.indexOf(agents.mode) + 1) % CYCLE.length]);
    else if (e.ctrlKey && e.key.toLowerCase() === "n") {
      if (!running && items.length > 0) agents.newChat(sid);
      inputRef.current?.focus();
    } else if (e.ctrlKey && e.key.toLowerCase() === "l") inputRef.current?.focus();
    else return false;
    return true;
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (keys.current(e)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const elapsed = running && session.started_at ? formatDuration(now - session.started_at) : "";
  const stepElapsed = stepTime(session, now);
  const modeInfo = MODES.find((m) => m.id === agents.mode) ?? MODES[0];

  return (
    <div className={`code ${reckless ? "is-reckless" : ""}`}>
      {/* ---- feed ---- */}
      <div className="feed-wrap">
        <div
          className="feed"
          ref={feedRef}
          onScroll={(e) => {
            const el = e.currentTarget;
            const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
            stick.current = atBottom;
            setScrolledUp(!atBottom);
          }}
        >
          {items.length === 0 && !running ? (
            <div className="empty-state">
              <p className="empty-title">
                What should Claude do in <b>{basename(session.project)}</b>?
              </p>
              <div className="suggestions">
                {SUGGESTIONS.map((s) => (
                  <button key={s} className="suggestion" title="Send now" onClick={() => send(s)}>
                    {s}
                  </button>
                ))}
              </div>
              <KeyHints />
            </div>
          ) : (
            <Feed
              items={items}
              running={running}
              onAllow={(rules) => agents.send(sid, "Allowed. Please continue.", rules)}
              onRetry={() => lastPrompt && send(lastPrompt)}
            />
          )}
          {queue.length > 0 && (
            <div className="queue">
              {queue.map((q, i) => (
                <div key={i} className="queued">
                  <span className="queued-text">{q}</span>
                  <button className="queued-x" title="Don't send this" onClick={() => agents.unqueue(sid, i)}>
                    <CrossIcon />
                  </button>
                </div>
              ))}
              <span className="queue-note">Sent as soon as Claude finishes (commands always wait for that)</span>
            </div>
          )}
        </div>
        {scrolledUp && (
          <button
            className="jump-latest jump-feed"
            onClick={() => {
              const el = feedRef.current;
              stick.current = true;
              el?.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
            }}
          >
            ↓ Latest
          </button>
        )}
      </div>

      {agents.error && <p className="error-text">{agents.error}</p>}
      {imageError && <p className="error-text">{imageError}</p>}

      <VoiceStrip voice={voice} />

      {/* ---- what's happening ---- */}
      {running && (
        <div className={`run-strip ${reckless ? "is-auto" : ""}`}>
          <span className="mini-spinner" />
          {reckless && <span className="auto-tag">Auto</span>}
          <span className="run-current shimmer">{session.current ?? "Thinking…"}</span>
          {stepElapsed && (
            <span className="run-step-time" title="How long this step has been running">
              {stepElapsed}
            </span>
          )}
          <span className="run-time" title="Since the message was sent">
            {stepElapsed ? `total ${elapsed}` : elapsed}
          </span>
          <button className="btn secondary sm" title="Stop (Esc)" onClick={() => agents.stop(sid)}>
            Stop <span className="kbd">Esc</span>
          </button>
        </div>
      )}

      {/* ---- composer: message + how Claude should handle it ---- */}
      <form
        className={`composer ${running ? "is-running" : ""} ${dropping ? "is-dropping" : ""}`}
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        onClick={() => inputRef.current?.focus()}
      >
        {images.length > 0 && (
          <div className="attachments" onClick={(e) => e.stopPropagation()}>
            {images.map((img) => (
              <span key={img.path} className="attachment" title={img.path}>
                {img.preview ? <img src={img.preview} alt="" /> : <ImageIcon />}
                <span className="attachment-name">{basename(img.path)}</span>
                <button
                  type="button"
                  className="queued-x"
                  title="Remove"
                  onClick={() => {
                    if (img.preview) URL.revokeObjectURL(img.preview);
                    setImages((prev) => prev.filter((i) => i.path !== img.path));
                  }}
                >
                  <CrossIcon />
                </button>
              </span>
            ))}
          </div>
        )}
        {slashMatches.length > 0 && (
          <SlashMenu commands={slashMatches} index={Math.min(slashIndex, slashMatches.length - 1)} onHover={setSlashIndex} onPick={(c) => pickCommand(c, true)} />
        )}
        <div className="chat-bar">
          <textarea
            onPaste={onPaste}
            ref={inputRef}
            rows={1}
            value={draft}
            placeholder={
              running
                ? gemini
                  ? "Write the next message — it goes out when Gemini finishes"
                  : "Add more details — Claude reads it at its next step"
                : lastPrompt
                  ? "Reply, or ask for something else…"
                  : `Ask ${agentName} to build, fix or explain something…`
            }
            onChange={(e) => setDraft(e.target.value)}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            onKeyDown={(e) => {
              if (slashMatches.length > 0) {
                const n = slashMatches.length;
                const move = (i: number) => {
                  e.preventDefault();
                  setSlashIndex(i);
                  setSlashMoved(true);
                };
                if (e.key === "ArrowDown") return move((slashIndex + 1) % n);
                if (e.key === "ArrowUp") return move((slashIndex - 1 + n) % n);
                const pick = slashMatches[Math.min(slashIndex, n - 1)];
                // Enter on a command the menu doesn't know yet ("/usage" before Claude's
                // first reply lists its own) sends it as typed, not a match by description
                const typed = e.key === "Enter" && !slashMoved && !namedLike(pick, slashQuery ?? "");
                if (!typed && ((e.key === "Enter" && !e.shiftKey) || (e.key === "Tab" && !e.shiftKey && !e.ctrlKey))) {
                  e.preventDefault();
                  e.stopPropagation();
                  return pickCommand(pick, e.key === "Enter");
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  return setSlashDismissed(true);
                }
              }
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submit();
              } else if (e.key === "Escape" && running) {
                // here Esc stops Claude instead of closing the notch
                e.preventDefault();
                e.stopPropagation();
                agents.stop(sid);
              } else if (e.key === "ArrowUp" && !draft && lastPrompt) {
                e.preventDefault();
                setDraft(lastPrompt);
              }
            }}
          />
          <div className="chat-tools" onClick={(e) => e.stopPropagation()}>
            <button
              type="button"
              className="tool-btn"
              title="Attach images (or paste / drop them here)"
              aria-label="Attach images"
              disabled={pickingImages}
              onClick={pickImages}
            >
              <ImageIcon />
            </button>

            <MicButton voice={voice} />

            {running && !draft.trim() && images.length === 0 ? (
              <button type="button" className="send-btn is-stop" title="Stop (Esc)" aria-label="Stop" onClick={() => agents.stop(sid)}>
                <span className="stop-sq" />
              </button>
            ) : (
              <button
                type="submit"
                className={`send-btn ${running ? "is-queue" : ""}`}
                title={running ? "Send (Enter): Claude reads it at its next step" : "Send (Enter)"}
                aria-label="Send"
                disabled={!draft.trim() && images.length === 0}
              >
                <SendIcon />
              </button>
            )}
          </div>
        </div>
        <div className="composer-foot" onClick={(e) => e.stopPropagation()}>
          <div className="seg modes" role="radiogroup" aria-label="What Claude may do">
            {MODES.map((m) => (
              <button
                key={m.id}
                type="button"
                role="radio"
                aria-checked={agents.mode === m.id}
                className={`${agents.mode === m.id ? "on" : ""} mode-${m.id}`}
                title={m.title}
                onClick={() => agents.setMode(m.id)}
              >
                {m.icon === "flame" ? <span className="auto-dot" /> : <StepGlyph icon={m.icon} />}
                {m.label}
              </button>
            ))}
          </div>

          <span className="mode-hint">{running && draft.trim() ? "Enter sends it — Claude takes it in at its next step" : modeInfo.hint}</span>

          {session.caveman && (
            <span
              className="caveman-badge"
              title={`Caveman ${session.caveman}: Claude answers tersely and uses far fewer output tokens. Code, commits and security notes stay normal. "/caveman lite" or "stop caveman" changes it for one message.`}
            >
              Caveman {session.caveman}
            </span>
          )}

          {gemini ? (
            <span className="agent-badge is-gemini" title={`Gemini CLI${session.model ? ` — ${session.model}` : ""}. Switch the agent in the folder menu above.`}>
              Gemini{session.model ? ` · ${session.model.replace(/^gemini-/, "")}` : ""}
            </span>
          ) : (
          <div className="model">
            <button
              type="button"
              className={`model-btn ${modelOpen ? "open" : ""}`}
              title={`Model${session.model ? ` — last run: ${session.model}` : ""}`}
              onClick={() => setModelOpen(!modelOpen)}
            >
              <span className="model-dot" />
              <span className="model-name">{MODELS.find((m) => m.id === agents.model)?.label ?? agents.model}</span>
              <ChevronIcon open={!modelOpen} />
            </button>
            {modelOpen && (
              <>
                <div className="menu-scrim" onClick={() => setModelOpen(false)} />
                <div className="menu menu-up menu-right" role="menu">
                  {MODELS.map((m) => (
                    <button
                      key={m.id}
                      type="button"
                      className={`menu-item ${m.id === agents.model ? "is-current" : ""}`}
                      role="menuitem"
                      onClick={() => {
                        agents.setModel(m.id);
                        setModelOpen(false);
                        inputRef.current?.focus();
                      }}
                    >
                      <span className="menu-title">{m.label}</span>
                      <span className="menu-sub">{m.title}</span>
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
          )}
        </div>
      </form>
    </div>
  );
}

function SlashMenu({ commands, index, onHover, onPick }: { commands: SlashCommand[]; index: number; onHover: (i: number) => void; onPick: (c: SlashCommand) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    ref.current?.querySelector(".is-current")?.scrollIntoView({ block: "nearest" });
  }, [index]);
  return (
    <div className="menu menu-up slash-menu" role="listbox" ref={ref} onClick={(e) => e.stopPropagation()}>
      {commands.map((c, i) => (
        <button
          key={c.name}
          type="button"
          role="option"
          aria-selected={i === index}
          className={`menu-item ${i === index ? "is-current" : ""}`}
          // keep the focus in the chat bar, or the menu closes before the click lands
          onMouseDown={(e) => e.preventDefault()}
          onMouseEnter={() => onHover(i)}
          onClick={() => onPick(c)}
        >
          <span className="slash-head">
            <span className="menu-title">/{c.name}</span>
            {c.hint && <span className="slash-hint">{c.hint}</span>}
            <span className="slash-source">{c.source}</span>
          </span>
          {c.description && <span className="menu-sub">{c.description}</span>}
        </button>
      ))}
    </div>
  );
}

function KeyHints() {
  return (
    <div className="key-hints">
      <span>
        <kbd>Enter</kbd> send
      </span>
      <span>
        <kbd>Shift</kbd>+<kbd>Tab</kbd> mode
      </span>
      <span>
        <kbd>↑</kbd> last message
      </span>
      <span>
        <kbd>/</kbd> commands
      </span>
      <span>
        <kbd>Esc</kbd> stop
      </span>
      <span>
        <kbd>Ctrl</kbd>+<kbd>N</kbd> new chat
      </span>
      <span>
        <kbd>Ctrl</kbd>+<kbd>T</kbd> new session
      </span>
      <span>
        <kbd>Ctrl</kbd>+<kbd>`</kbd> terminal
      </span>
    </div>
  );
}

// ---- feed rendering ---------------------------------------------------------------

type Step = Extract<FeedItem, { type: "step" }>;
type Done = Extract<FeedItem, { type: "done" }>;

type Row = { kind: "item"; item: FeedItem; key: string } | { kind: "group"; icon: StepIcon; steps: Step[]; key: string };

/** Runs of 2+ consecutive reads/searches collapse into one "Read 4 files" row. */
function toRows(items: FeedItem[]): Row[] {
  const rows: Row[] = [];
  items.forEach((item, i) => {
    const prev = rows[rows.length - 1];
    if (item.type === "step" && (item.icon === "read" || item.icon === "search")) {
      if (prev?.kind === "group" && prev.icon === item.icon) {
        prev.steps.push(item);
        return;
      }
      if (prev?.kind === "item" && prev.item.type === "step" && prev.item.icon === item.icon) {
        rows[rows.length - 1] = { kind: "group", icon: item.icon, steps: [prev.item, item], key: prev.key };
        return;
      }
    }
    rows.push({ kind: "item", item, key: `${item.type}-${item.type === "step" ? item.id : i}` });
  });
  return rows;
}

/** One message and everything Claude did for it. */
interface Run {
  run: number;
  prompt: string | null;
  body: FeedItem[];
  done: Done | null;
}

function toRuns(items: FeedItem[]): Run[] {
  const runs: Run[] = [];
  for (const item of items) {
    let r = runs[runs.length - 1];
    if (!r || r.run !== item.run) {
      r = { run: item.run, prompt: null, body: [], done: null };
      runs.push(r);
    }
    if (item.type === "prompt") r.prompt = item.text;
    else if (item.type === "done") r.done = item;
    else r.body.push(item);
  }
  return runs;
}

function Feed({ items, running, onAllow, onRetry }: { items: FeedItem[]; running: boolean; onAllow: (rules: string[]) => void; onRetry: () => void }) {
  const runs = useMemo(() => toRuns(items), [items]);
  const last = runs[runs.length - 1];
  const thinking = running && !items.some((i) => i.type === "step" && i.state === "running");
  return (
    <>
      {runs.map((r) => (
        <RunBlock key={r.run} run={r} latest={r === last} canAct={!running && r === last} onAllow={onAllow} onRetry={onRetry} />
      ))}
      {thinking && (
        <div className="thinking">
          <span className="mini-spinner" /> Thinking…
        </div>
      )}
    </>
  );
}

/** Older runs fold to their final answer; the steps are one click away. */
function RunBlock({ run, latest, canAct, onAllow, onRetry }: { run: Run; latest: boolean; canAct: boolean; onAllow: (rules: string[]) => void; onRetry: () => void }) {
  const [open, setOpen] = useState(false);
  const steps = run.body.filter((i) => i.type === "step").length;
  const texts = run.body.filter((i): i is Extract<FeedItem, { type: "text" }> => i.type === "text");
  const folded = !latest && !open && steps > 0;
  const rows = useMemo(() => toRows(run.body), [run.body]);
  return (
    <div className={`run ${latest ? "is-latest" : ""}`}>
      {run.prompt != null && <Prompt text={run.prompt} />}
      {!latest && steps > 0 && (
        <button className={`run-fold ${open ? "is-open" : ""}`} onClick={() => setOpen(!open)}>
          <ChevronIcon open={open} />
          {open ? "Hide steps" : `${steps} step${steps === 1 ? "" : "s"}${texts.length > 1 ? ` · ${texts.length - 1} note${texts.length === 2 ? "" : "s"}` : ""}`}
        </button>
      )}
      {folded
        ? texts.length > 0 && <RichText text={texts[texts.length - 1].text} />
        : rows.map((row) =>
            row.kind === "group" ? (
              <GroupRow key={row.key} icon={row.icon} steps={row.steps} />
            ) : (
              <Fragment key={row.key}>
                {row.item.type === "followup" && <Prompt text={row.item.text} />}
                {row.item.type === "skills" && <SkillsRow skills={row.item.skills} />}
                {row.item.type === "text" && <RichText text={row.item.text} />}
                {row.item.type === "step" && <StepRow step={row.item} />}
              </Fragment>
            ),
          )}
      {run.done && <DoneRow done={run.done} canAct={canAct} onAllow={onAllow} onRetry={onRetry} />}
    </div>
  );
}

function Prompt({ text }: { text: string }) {
  const { text: body, images } = splitImages(text);
  return (
    <div className="prompt">
      {body}
      {images.length > 0 && (
        <div className="attachments">
          {images.map((p) => (
            <span key={p} className="attachment" title={p}>
              <ImageIcon />
              <span className="attachment-name">{basename(p)}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

/** The skills Claude started with: one line, the full list one click away. */
function SkillsRow({ skills }: { skills: string[] }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="step step-plan is-ok">
      <button className="step-head" title={skills.join("\n")} onClick={() => setOpen(!open)}>
        <span className="step-icon">
          <StepGlyph icon="plan" />
        </span>
        <span className="step-label">
          Started with {skills.length} skill{skills.length === 1 ? "" : "s"} · {skills.join(", ")}
        </span>
        <ChevronIcon open={open} />
      </button>
      {open && (
        <div className="step-body skills-list">
          {skills.map((s) => (
            <span key={s} className="code-chip" title={`/${s}`}>
              {s}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function StateMark({ state }: { state: "running" | "ok" | "error" }) {
  if (state === "running") return <span className="mini-spinner" />;
  return <span className={`mark mark-${state}`}>{state === "ok" ? <CheckIcon /> : <CrossIcon />}</span>;
}

function StepRow({ step }: { step: Step }) {
  const [open, setOpen] = useState(false);
  const expandable = !!(step.detail || step.output);
  return (
    <div className={`step step-${step.icon} is-${step.state}`}>
      <button className="step-head" disabled={!expandable} onClick={() => setOpen(!open)}>
        <span className="step-icon">
          <StepGlyph icon={step.icon} />
        </span>
        <span className="step-label">{step.label}</span>
        {expandable && <ChevronIcon open={open} />}
        <StepTime step={step} />
        <StateMark state={step.state} />
      </button>
      {open && (
        <div className="step-body">
          {step.detail && <pre className="step-detail">{step.detail}</pre>}
          {step.output && <pre className="step-output">{step.output}</pre>}
        </div>
      )}
    </div>
  );
}

/** Live while it runs, then how long it took (only for steps that take a while). */
function StepTime({ step }: { step: Step }) {
  const running = step.state === "running";
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [running]);
  const ms = running ? now - step.started : step.duration_ms;
  if (ms == null || ms < 3000) return null;
  return (
    <span className={`step-time ${running ? "is-live" : ""}`} title={running ? "Running for" : "Took"}>
      {running ? formatDuration(ms) : formatSpan(ms)}
    </span>
  );
}

function GroupRow({ icon, steps }: { icon: StepIcon; steps: Step[] }) {
  const [open, setOpen] = useState(false);
  const state = steps.some((s) => s.state === "running") ? "running" : steps.some((s) => s.state === "error") ? "error" : "ok";
  const label = icon === "read" ? `Read ${steps.length} files` : `Searched ${steps.length} times`;
  return (
    <div className={`step step-${icon} is-${state}`}>
      <button className="step-head" onClick={() => setOpen(!open)}>
        <span className="step-icon">
          <StepGlyph icon={icon} />
        </span>
        <span className="step-label">{label}</span>
        <ChevronIcon open={open} />
        <StateMark state={state} />
      </button>
      {open && (
        <div className="step-children">
          {steps.map((s) => (
            <StepRow key={s.id} step={s} />
          ))}
        </div>
      )}
    </div>
  );
}

function DoneRow({ done, canAct, onAllow, onRetry }: { done: Done; canAct: boolean; onAllow: (rules: string[]) => void; onRetry: () => void }) {
  return (
    <>
      {done.denied.length > 0 && (
        <div className="denied">
          <div className="who-row">
            <AlertIcon /> <span className="n">Claude</span> needs permission
          </div>
          <div className="denied-list">
            {done.denied.map((d) => (
              <span key={d.rule} className="code-chip" title={d.rule}>
                {d.label}
              </span>
            ))}
          </div>
          {canAct && (
            <div className="actions">
              <button className="btn primary sm" onClick={() => onAllow(done.denied.map((d) => d.rule))}>
                Allow and continue
              </button>
            </div>
          )}
        </div>
      )}
      <div className={`done ${done.stopped ? "is-stopped" : done.ok ? "is-ok" : "is-error"}`}>
        {done.stopped ? <span className="stop-sq" /> : done.ok ? <CheckIcon /> : <CrossIcon />}
        <span className="done-text">
          {done.stopped
            ? "Stopped"
            : done.ok
              ? `Done in ${formatSpan(done.duration_ms)}${done.steps ? ` · ${done.steps} step${done.steps === 1 ? "" : "s"}` : ""}`
              : (done.message ?? "Something went wrong")}
        </span>
        {canAct && (done.stopped || !done.ok) && (
          <button className="btn secondary xs" title="Send the same message again" onClick={onRetry}>
            <RetryIcon /> Retry
          </button>
        )}
      </div>
    </>
  );
}

/** Claude's text, as Markdown; `/usage`'s report as a card. */
function RichText({ text }: { text: string }) {
  const usage = useMemo(() => parseUsageReport(text), [text]);
  return (
    <div className={`answer ${usage ? "is-usage" : ""}`}>
      <CopyButton text={text} />
      {usage ? <UsageCard report={usage} /> : <Markdown text={text} />}
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className={`copy-btn ${copied ? "is-copied" : ""}`}
      title="Copy"
      aria-label="Copy"
      onClick={() => {
        navigator.clipboard
          .writeText(text)
          .then(() => {
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1200);
          })
          .catch(() => {});
      }}
    >
      {copied ? <CheckIcon /> : <CopyIcon />}
    </button>
  );
}
