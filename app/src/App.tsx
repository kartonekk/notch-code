import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { api, AppStatus, ClaudeStatus, Limit, on, SessionStatus, UpdateStatus } from "./api";
import { Agents, AgentNotice, CodeTab, stepTime, useAgents, Welcome } from "./CodeTab";
import { useVoice, voiceLine } from "./Voice";
import { Embers } from "./Reckless";
import { PILL_STEP, PILL_WIDTH, ReviewPills, useReview } from "./Review";
import { SPEAKER_LABEL, TranscribeTab, useTranscription } from "./TranscribeTab";
import {
  AlertIcon,
  Bars,
  basename,
  capitalize,
  CheckIcon,
  ChevronIcon,
  ClaudeBanner,
  CrossIcon,
  FolderIcon,
  formatDuration,
  IconButton,
  KeyIcon,
  MicIcon,
  needsClaudeAttention,
  PinIcon,
  PlusIcon,
  PowerIcon,
  StatusDot,
  store,
  MicOffIcon,
  UpdateBanner,
} from "./ui";
import "./notch.css";

// Notch sizes in logical pixels. The window itself always has the EXPANDED size
// (see WINDOW_W in lib.rs); the backend makes everything outside the current
// notch click-through and reports hover via the `notch-hover` event.
const COLLAPSED = { w: 280, h: 36 };
const COLLAPSED_LIVE = { w: 460, h: 36 };
const EXPANDED = { w: 680, h: 500 };
const ANIM_MS = 260;
/** "Later" on the Claude sign-in reminder hides it for this long */
const CLAUDE_SNOOZE_MS = 60 * 60 * 1000;

/** what the panel shows: a Claude session (by id) or the transcriber */
type View = number | "transcribe";

export default function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [claude, setClaude] = useState<ClaudeStatus | null>(null);
  const [claudeSnoozedUntil, setClaudeSnoozedUntil] = useState(0);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  /** build the user said "Later" to; a newer build shows the notice again */
  const [skippedBuild, setSkippedBuild] = useState<number | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [view, setViewState] = useState<View | null>(null);
  /** a view is in the middle of something (typing, choosing a folder) */
  const [hold, setHold] = useState(false);
  const [now, setNow] = useState(Date.now());

  const agents = useAgents(expanded, typeof view === "number" ? view : null);
  const review = useReview();
  const voice = useVoice();
  const rec = useTranscription(status, expanded && view === "transcribe");
  const { recording, paused, phase } = rec;
  const sessions = agents.sessions;
  const working = agents.running.length > 0;
  const reckless = agents.mode === "reckless";

  // ---- views ------------------------------------------------------------------
  const setView = (v: View) => {
    setViewState(v);
    // remembered by folder: session ids change between launches
    store.set("view", v === "transcribe" ? "transcribe" : (agents.session(v)?.project ?? ""));
  };
  // first load, and when the shown session gets closed: fall back to something that exists
  useEffect(() => {
    if (!agents.status) return;
    if (view === "transcribe" || (view != null && sessions.some((s) => s.id === view))) return;
    const saved = store.get("view");
    if (view == null && saved === "transcribe") {
      setViewState("transcribe");
      return;
    }
    const match = sessions.find((s) => s.project === saved) ?? sessions[0];
    setViewState(match ? match.id : null);
  }, [agents.status, view, sessions]);
  const current: SessionStatus | null = typeof view === "number" ? agents.session(view) : null;
  const codeView = view !== "transcribe";

  const openSession = async (path: string) => {
    const existing = sessions.find((s) => s.project.toLowerCase() === path.toLowerCase());
    if (existing) return setView(existing.id);
    const sid = await agents.open(path);
    if (sid != null) {
      setViewState(sid);
      store.set("view", path);
    }
  };
  const [picking, setPicking] = useState(false);
  const pickAndOpen = async () => {
    setPicking(true);
    const path = await agents.pickFolder(current?.project ?? null);
    setPicking(false);
    if (path) openSession(path);
  };

  // a spoken prompt goes to the session on screen (else the first one); queued while it works
  const voicePrompt = useRef<(text: string) => void>(() => {});
  voicePrompt.current = (text) => {
    const target = current ?? sessions[0] ?? null;
    if (!target) {
      agents.showFlash({ tone: "warn", text: "Heard you, but Claude has no folder open yet" });
      return;
    }
    agents.send(target.id, text);
    agents.showFlash({ tone: "ok", text: `${target.running ? "Queued for" : "Sent to"} ${basename(target.project)}: “${text}”` });
  };
  useEffect(() => {
    const sub = on<string>("voice-prompt", (text) => voicePrompt.current(text));
    return () => {
      sub.then((un) => un());
    };
  }, []);

  // a short shake + flash when reckless mode is switched on
  const [igniting, setIgniting] = useState(false);
  const wasReckless = useRef(reckless);
  useEffect(() => {
    if (reckless && !wasReckless.current) {
      setIgniting(true);
      const t = window.setTimeout(() => setIgniting(false), 800);
      wasReckless.current = reckless;
      return () => window.clearTimeout(t);
    }
    wasReckless.current = reckless;
  }, [reckless]);

  // ---- backend wiring -------------------------------------------------------
  useEffect(() => {
    api.status().then(setStatus).catch(() => {});
    api.claudeStatus().then(setClaude).catch(() => {});
    api.updateStatus().then(setUpdate).catch(() => {});
    const subs = [
      on<AppStatus>("status", setStatus),
      on<ClaudeStatus>("claude", setClaude),
      on<UpdateStatus>("update", setUpdate),
    ];
    return () => subs.forEach((p) => p.then((un) => un()));
  }, []);

  useEffect(() => {
    if (!working) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [working]);

  // ---- notch geometry -------------------------------------------------------
  const collapseTimer = useRef<number | null>(null);
  const hoverTimer = useRef<number | null>(null);
  /** the pointer is on a review notification: hovering there must not open the notch */
  const overPill = useRef(false);
  /** the pointer is on the notch itself (the hit area is wider while notifications show) */
  const overNotch = useRef(false);
  const pills = !expanded ? Math.min(3, review.pending.length) : 0;
  const notices = Object.values(agents.notices);
  const notice: AgentNotice | null =
    notices.length === 0
      ? null
      : notices.length === 1
        ? notices[0]
        : {
            tone: notices.some((n) => n.tone === "error") ? "error" : notices.some((n) => n.tone === "warn") ? "warn" : "ok",
            text: `${notices.length} sessions finished`,
          };
  // finished stays on until the notch is opened once
  const finished = !expanded && !working && !!notice;
  const flash = !expanded ? agents.flash : null;
  const live = recording || working || finished || pills > 0 || voice.active || !!flash;
  const collapsedSize = live ? COLLAPSED_LIVE : COLLAPSED;

  const clearTimers = () => {
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    collapseTimer.current = hoverTimer.current = null;
  };

  const expand = useCallback(async () => {
    clearTimers();
    await api.setNotchSize(EXPANDED.w, EXPANDED.h);
    setExpanded(true);
    api.claudeRefresh().catch(() => {});
  }, []);

  const collapse = useCallback(() => {
    clearTimers();
    setExpanded(false);
  }, []);

  const keepOpen = pinned || hold || picking || rec.ending;
  const onEnter = () => {
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    collapseTimer.current = null;
    if (!expanded && !hoverTimer.current)
      hoverTimer.current = window.setTimeout(() => {
        hoverTimer.current = null;
        if (!overPill.current && (pills === 0 || overNotch.current)) expand();
      }, 140);
  };
  const onLeave = () => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    hoverTimer.current = null;
    if (expanded && !keepOpen) collapseTimer.current = window.setTimeout(collapse, 600);
  };

  // Hover comes from the backend's cursor polling; the handler ref keeps the
  // once-registered listener seeing current state.
  const hoverHandler = useRef<(inside: boolean) => void>(() => {});
  hoverHandler.current = (inside) => (inside ? onEnter() : onLeave());
  useEffect(() => {
    const sub = on<boolean>("notch-hover", (inside) => hoverHandler.current(inside));
    return () => {
      sub.then((un) => un());
    };
  }, []);

  // `shell` keeps the expanded panel mounted (and the hit area large) until the
  // collapse animation has finished, then the hit area shrinks to the bar.
  const [shell, setShell] = useState(expanded);
  useEffect(() => {
    if (expanded) setShell(true);
    else {
      const t = window.setTimeout(() => setShell(false), ANIM_MS + 40);
      return () => window.clearTimeout(t);
    }
  }, [expanded]);
  const hitWidth = pills > 0 ? Math.max(collapsedSize.w, PILL_WIDTH) : collapsedSize.w;
  const hitHeight = collapsedSize.h + (pills > 0 ? 8 + pills * PILL_STEP : 0);
  useEffect(() => {
    if (!shell && !expanded) api.setNotchSize(hitWidth, hitHeight);
  }, [shell, expanded, hitWidth, hitHeight]);

  // ---- keyboard ---------------------------------------------------------------
  const keys = useRef<(e: KeyboardEvent) => boolean>(() => false);
  keys.current = (e) => {
    const n = Number(e.key);
    if (e.ctrlKey && n >= 1 && n <= 9 && sessions[n - 1]) setView(sessions[n - 1].id);
    else if (e.ctrlKey && e.key.toLowerCase() === "m") setView("transcribe");
    else if (e.ctrlKey && e.key.toLowerCase() === "t") pickAndOpen();
    else if (e.ctrlKey && e.key === "Tab") {
      const order: View[] = [...sessions.map((s) => s.id), "transcribe"];
      const i = view == null ? -1 : order.indexOf(view);
      setView(order[(i + (e.shiftKey ? order.length - 1 : 1)) % order.length]);
    } else if (e.key === "Escape" && !pinned && !rec.ending) collapse();
    else return false;
    return true;
  };
  useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => {
      if (keys.current(e)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [expanded]);

  // ---- derived ------------------------------------------------------------------
  const size = expanded ? EXPANDED : collapsedSize;
  const claudeAlert = needsClaudeAttention(claude);
  const claudeBusy = claude?.state === "signing_in" || claude?.state === "installing";
  const showClaudeBanner = claudeBusy || (claudeAlert && Date.now() >= claudeSnoozedUntil);
  const showUpdate =
    update?.state === "downloading" ||
    ((update?.state === "available" || update?.state === "error") && update.available !== skippedBuild);
  const limit = agents.limit;
  const limitHit = limit?.status === "rejected" && (limit.resets_at ?? 0) > Date.now();
  // the session the collapsed bar talks about: the one you look at, else the first busy one
  const busy = agents.running.find((s) => s.id === view) ?? agents.running[0];
  const busyText = busy
    ? `${agents.running.length > 1 ? `${agents.running.length} working · ` : ""}${basename(busy.project)}: ${busy.current ?? "Working…"}`
    : "";
  const busyElapsed = busy?.started_at ? formatDuration(now - busy.started_at) : "";
  const busyStep = busy ? stepTime(busy, now) : "";
  // when nothing is going on the notch stays empty; only things that need you show up
  const idleMessage =
    phase === "preparing" && status?.progress != null
      ? `Downloading speech model ${Math.round(status.progress * 100)}%`
      : claudeAlert
        ? claude?.state === "not_installed"
          ? "Claude isn't installed"
          : "Claude needs sign-in"
        : limitHit
          ? `5-hour limit reached · resets ${clock(limit!.resets_at!)}`
          : phase === "error"
            ? "Speech model error — hover"
            : null;

  return (
    <div className="stage">
      <div
        className={[
          "notch",
          expanded && "is-expanded",
          `phase-${phase}`,
          reckless && "is-reckless",
          working && "is-working",
          igniting && "is-igniting",
          finished && notice && `is-finished tone-${notice.tone}`,
        ]
          .filter(Boolean)
          .join(" ")}
        style={{ width: size.w, height: size.h }}
        onClick={() => !expanded && expand()}
        onPointerEnter={() => (overNotch.current = true)}
        onPointerLeave={() => (overNotch.current = false)}
      >
        <span className="ear ear-l" />
        <span className="ear ear-r" />
        <div className="notch-clip">
          {/* ---------- collapsed bar ---------- */}
          <div className="bar" onPointerEnter={() => !expanded && onEnter()}>
            {voice.active ? (
              <>
                <span className="voice-wave" aria-hidden>
                  <i />
                  <i />
                  <i />
                </span>
                <span className={`ticker ${voice.phase === "hearing" && voice.text ? "" : "muted"}`}>{voiceLine(voice)}</span>
              </>
            ) : flash ? (
              <>
                <span className={`notice-icon is-${flash.tone}`}>
                  {flash.tone === "ok" ? <CheckIcon /> : flash.tone === "warn" ? <AlertIcon /> : <CrossIcon />}
                </span>
                <span className="ticker">{flash.text}</span>
              </>
            ) : working && recording ? (
              <>
                <StatusDot phase={paused ? "paused" : "recording"} />
                <span className={`timer ${paused ? "is-paused" : ""}`}>{rec.elapsed}</span>
                <span className="mini-spinner agent-spin" />
                <span className="ticker">{busyText}</span>
                {busyStep && <span className="bar-step-time">{busyStep}</span>}
              </>
            ) : working ? (
              <>
                <span className="mini-spinner agent-spin" />
                <span className="ticker">{busyText}</span>
                {busyStep && <span className="bar-step-time">{busyStep}</span>}
                <span className="bar-time">{busyElapsed}</span>
              </>
            ) : recording ? (
              <>
                <StatusDot phase={paused ? "paused" : "recording"} />
                <span className={`timer ${paused ? "is-paused" : ""}`}>{rec.elapsed}</span>
                <span className="ticker">
                  {paused ? (
                    <span className="muted">Paused — hover to resume</span>
                  ) : rec.lastLine ? (
                    <>
                      <b>{SPEAKER_LABEL[rec.lastLine.speaker]}:</b> {rec.lastLine.text}
                    </>
                  ) : (
                    <span className="muted">Listening…</span>
                  )}
                </span>
                <Bars level={paused ? 0 : Math.max(rec.levels.app, rec.levels.mic)} />
              </>
            ) : finished && notice ? (
              <>
                <span className={`notice-icon is-${notice.tone}`}>
                  {notice.tone === "ok" ? <CheckIcon /> : notice.tone === "warn" ? <AlertIcon /> : <CrossIcon />}
                </span>
                <span className="ticker">{notice.text}</span>
                <span className="bar-hint">hover to see</span>
              </>
            ) : idleMessage ? (
              <>
                <span className="bar-lead">
                  <StatusDot phase={claudeAlert || limitHit ? "error" : phase} />
                  {claudeAlert && (
                    <span className="claude-alert" title={claude?.message ?? "Claude needs attention"}>
                      <KeyIcon />
                    </span>
                  )}
                </span>
                <span className="bar-label">{idleMessage}</span>
              </>
            ) : null}
          </div>

          {/* ---------- expanded panel ---------- */}
          {shell && (
            <div className="panel" onClick={(e) => e.stopPropagation()}>
              {reckless && <Embers fast={working} />}
              <Rail
                agents={agents}
                view={view}
                recording={recording}
                paused={paused}
                picking={picking}
                onView={setView}
                onOpen={openSession}
                onPick={pickAndOpen}
                onHold={setHold}
              />
              <div className="main">
                <header className="panel-head">
                  {codeView && current ? (
                    <ProjectMenu agents={agents} session={current} onPick={pickAndOpen} onHold={setHold} />
                  ) : (
                    <span className="view-title">{codeView ? "Claude Code" : "Transcribe"}</span>
                  )}
                  {codeView && current && (
                    <button
                      className="new-chat"
                      title="New conversation (Ctrl+N)"
                      disabled={current.running || agents.feed(current.id).length === 0}
                      onClick={() => agents.newChat(current.id)}
                    >
                      <PlusIcon /> New chat
                    </button>
                  )}
                  <div className="head-actions">
                    {codeView && claude?.state === "signed_in" && <LimitChip plan={claude.plan} limit={agents.limit} />}
                    {!codeView && status?.backend && <span className="chip">{status.backend}</span>}
                    {!codeView && (
                      <IconButton title="Transcripts folder" onClick={() => api.openFolder().catch(() => {})}>
                        <FolderIcon />
                      </IconButton>
                    )}
                    <IconButton
                      title={
                        voice.wake
                          ? "Voice on: say “Claude, …” or use the mic button; sent when you stop talking. Click to turn off"
                          : "Voice off: the microphone stays closed. Click to turn on"
                      }
                      active={voice.wake}
                      onClick={() => voice.setWake(!voice.wake)}
                    >
                      {voice.wake ? <MicIcon /> : <MicOffIcon />}
                    </IconButton>
                    <IconButton title={pinned ? "Unpin" : "Keep open"} active={pinned} onClick={() => setPinned(!pinned)}>
                      <PinIcon />
                    </IconButton>
                    <IconButton title="Quit Notch Code" onClick={() => api.quit()}>
                      <PowerIcon />
                    </IconButton>
                  </div>
                </header>

                {showClaudeBanner && claude && (
                  <ClaudeBanner claude={claude} onLater={() => setClaudeSnoozedUntil(Date.now() + CLAUDE_SNOOZE_MS)} />
                )}
                {showUpdate && update && (
                  <UpdateBanner update={update} recording={recording} onLater={() => setSkippedBuild(update.available)} />
                )}

                <div className="tab-body">
                  {!codeView ? (
                    <TranscribeTab t={rec} status={status} />
                  ) : current ? (
                    <CodeTab key={current.id} agents={agents} session={current} voice={voice} onHold={setHold} />
                  ) : (
                    <Welcome agents={agents} onOpen={openSession} />
                  )}
                </div>
              </div>
            </div>
          )}
        </div>

        {pills > 0 && (
          <ReviewPills
            review={review}
            busy={(p) => sessions.some((s) => s.running && s.project === p)}
            onHover={(over) => (overPill.current = over)}
          />
        )}
      </div>
    </div>
  );
}

/** "14:30" */
function clock(ms: number) {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** two letters for a folder, e.g. "notch-transcript" -> "NT" */
function initials(path: string) {
  const words = basename(path).split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  const s = words.length > 1 ? words[0][0] + words[1][0] : (words[0] ?? "?").slice(0, 2);
  return s.toUpperCase();
}

function hue(path: string) {
  let h = 0;
  for (const c of path.toLowerCase()) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

/**
 * The left edge of the panel: one button per Claude session (its state at a
 * glance), + for another folder, and the microphone for transcription.
 */
function Rail({
  agents,
  view,
  recording,
  paused,
  picking,
  onView,
  onOpen,
  onPick,
  onHold,
}: {
  agents: Agents;
  view: View | null;
  recording: boolean;
  paused: boolean;
  picking: boolean;
  onView: (v: View) => void;
  onOpen: (path: string) => void;
  onPick: () => void;
  onHold: (hold: boolean) => void;
}) {
  const [adding, setAdding] = useState(false);
  useEffect(() => onHold(adding), [adding, onHold]);
  const openPaths = new Set(agents.sessions.map((s) => s.project.toLowerCase()));
  const recent = agents.recent.filter((p) => !openPaths.has(p.toLowerCase()));
  return (
    <nav className="rail">
      {agents.sessions.map((s, i) => {
        const tone = agents.unseen[s.id];
        const queued = agents.queue(s.id).length;
        return (
          <div key={s.id} className="rail-slot">
            <button
              className={`rail-btn session ${view === s.id ? "is-active" : ""} ${s.running ? "is-running" : ""}`}
              style={{ "--h": hue(s.project) } as CSSProperties}
              title={`${basename(s.project)}${s.running ? ` — ${s.current ?? "working"}` : ""}\n${s.project}\nCtrl+${i + 1}`}
              onClick={() => onView(s.id)}
            >
              {initials(s.project)}
              {tone && !s.running && <span className={`rail-badge is-${tone}`} />}
              {queued > 0 && <span className="rail-count">{queued}</span>}
            </button>
            <button
              className="rail-close"
              title={s.running ? "Stop Claude and close this session" : "Close this session"}
              onClick={() => agents.close(s.id)}
            >
              <CrossIcon />
            </button>
          </div>
        );
      })}

      <div className="rail-add">
        <button
          className={`rail-btn add ${adding ? "is-open" : ""}`}
          title="New session in another folder (Ctrl+T)"
          disabled={picking || agents.sessions.length >= 9}
          onClick={() => (recent.length ? setAdding(!adding) : onPick())}
        >
          <PlusIcon />
        </button>
        {adding && (
          <>
            <div className="menu-scrim" onClick={() => setAdding(false)} />
            <div className="menu menu-side" role="menu">
              {recent.map((p) => (
                <button
                  key={p}
                  className="menu-item"
                  role="menuitem"
                  onClick={() => {
                    setAdding(false);
                    onOpen(p);
                  }}
                >
                  <span className="menu-title">{basename(p)}</span>
                  <span className="menu-sub">{p}</span>
                </button>
              ))}
              <div className="menu-sep" />
              <button
                className="menu-item"
                role="menuitem"
                onClick={() => {
                  setAdding(false);
                  onPick();
                }}
              >
                <span className="menu-title">Choose another folder…</span>
              </button>
            </div>
          </>
        )}
      </div>

      <div className="rail-spacer" />

      <button
        className={`rail-btn mic ${view === "transcribe" ? "is-active" : ""} ${recording ? (paused ? "is-paused" : "is-recording") : ""}`}
        title="Transcribe (Ctrl+M)"
        onClick={() => onView("transcribe")}
      >
        <MicIcon />
      </button>
    </nav>
  );
}

/** The session's folder: open it, move the session, start another, close it. */
function ProjectMenu({ agents, session, onPick, onHold }: { agents: Agents; session: SessionStatus; onPick: () => void; onHold: (hold: boolean) => void }) {
  const [open, setOpen] = useState(false);
  useEffect(() => onHold(open), [open, onHold]);
  const others = agents.recent.filter((p) => p.toLowerCase() !== session.project.toLowerCase());
  const queued = agents.queue(session.id).length;
  return (
    <div className="project">
      <button className={`project-btn ${open ? "is-open" : ""}`} title={session.project} onClick={() => setOpen(!open)}>
        <FolderIcon size={13} />
        <span className="project-name">{basename(session.project)}</span>
        <ChevronIcon open={open} />
      </button>
      <span className="session-label">
        {session.running ? `working${queued ? ` · ${queued} queued` : ""}` : session.has_session ? "conversation" : "new conversation"}
      </span>
      {open && (
        <>
          <div className="menu-scrim" onClick={() => setOpen(false)} />
          <div className="menu" role="menu">
            <button
              className="menu-item"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                api.agentOpenFolder(session.project).catch(() => {});
              }}
            >
              <span className="menu-title">Open in Explorer</span>
              <span className="menu-sub">{session.project}</span>
            </button>
            {!session.running && others.length > 0 && <div className="menu-sep" />}
            {!session.running &&
              others.slice(0, 5).map((p) => (
                <button
                  key={p}
                  className="menu-item"
                  role="menuitem"
                  onClick={() => {
                    setOpen(false);
                    agents.moveTo(session.id, p);
                  }}
                >
                  <span className="menu-title">Switch this session to {basename(p)}</span>
                  <span className="menu-sub">{p}</span>
                </button>
              ))}
            <div className="menu-sep" />
            <button
              className="menu-item"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                onPick();
              }}
            >
              <span className="menu-title">New session in another folder… (Ctrl+T)</span>
            </button>
            <button
              className="menu-item is-danger"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                agents.close(session.id);
              }}
            >
              <span className="menu-title">{session.running ? "Stop and close this session" : "Close this session"}</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
}

/** Plan + the 5-hour usage window (as far as Claude has reported it). */
function LimitChip({ plan, limit }: { plan: string | null; limit: Limit | null }) {
  const current = !!limit && ((limit.resets_at ?? 0) > Date.now() || limit.status === "rejected");
  const used = current ? limit!.used : null;
  const tone =
    limit?.status === "rejected" && current ? "is-error" : current && (limit!.status === "allowed_warning" || (used ?? 0) >= 0.8) ? "is-warn" : "";
  const reset = current && limit!.resets_at ? clock(limit!.resets_at) : null;
  const left = used != null ? Math.max(0, Math.round((1 - used) * 100)) : null;
  const title = current
    ? `5-hour limit: ${left != null ? `${left}% left (${Math.round(used! * 100)}% used)` : limit!.status === "rejected" ? "reached" : "usage not reported yet"}${reset ? ` · resets at ${reset}` : ""}`
    : "5-hour limit: shown after Claude's next reply";
  return (
    <span className={`chip limit-chip ${tone}`} title={title}>
      Claude{plan ? ` · ${capitalize(plan)}` : ""}
      {current && (
        <>
          <span className="limit-sep" />
          5h
          {left != null && (
            <span className="limit-meter">
              <i style={{ width: `${left}%` }} />
            </span>
          )}
          {left != null ? `${left}% left` : limit!.status === "rejected" ? "limit reached" : ""}
          {reset && <span className="limit-reset">↻ {reset}</span>}
        </>
      )}
    </span>
  );
}
