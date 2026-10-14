import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { api, AppStatus, ClaudeStatus, GeminiUsage, Limit, on, Provider, PROVIDER_NAME, SessionStatus, UpdateStatus, Usage } from "./api";
import { Agents, AgentNotice, CodeTab, stepTime, useAgents } from "./CodeTab";
import { Home, homePills } from "./Home";
import { useMedia } from "./Media";
import { NotchMascot, useMascot } from "./Mascot";
import { Settings, useGemini } from "./Settings";
import { geminiReport, UsageCard } from "./Usage";
import { useVoice, voiceLine } from "./Voice";
import { PILL_STEP, PILL_WIDTH, ReviewPills, useReview } from "./Review";
import { SPEAKER_LABEL, TranscribeTab, useTranscription } from "./TranscribeTab";
import {
  AgentTag,
  AlertIcon,
  basename,
  capitalize,
  CheckIcon,
  ChevronIcon,
  ClaudeBanner,
  CrossIcon,
  formatDuration,
  GearIcon,
  HomeIcon,
  hue,
  IconButton,
  KeyIcon,
  MicIcon,
  Mini,
  needsClaudeAttention,
  Orb,
  PinIcon,
  PlusIcon,
  PowerIcon,
  Status,
  store,
  TerminalIcon,
  MicOffIcon,
  toneStatus,
  UpdateBanner,
  Bars,
} from "./ui";
import "./notch.css";

// Notch sizes in logical pixels. The window itself always has the largest
// expanded size (see WINDOW_W / WINDOW_H in lib.rs); the backend makes everything
// outside the current notch click-through and reports hover via `notch-hover`.
const COMPACT = { w: 288, h: 32 };
const COMPACT_LIVE = { w: 440, h: 32 };
const EXPANDED_W = 640;
/** the tallest the expanded notch gets (WINDOW_H in lib.rs is this + its margin) */
export const EXPANDED_MAX_H = 500;
/** padding (8 + 10) and the 34 px header around a view */
const CHROME_H = 52;
const BANNER_H = 38;
const PILL_ROW = 32;
/** tall enough for the + menu under the tabs */
const TAB_MENU_H = 340;
const ANIM_MS = 300;
/** the settings view, header included */
const SETTINGS_H = 444;
/** the now-playing card under Home's status card (+ the gap above it) */
const MEDIA_H = 74;
/** "Later" on the Claude sign-in reminder hides it for this long */
const CLAUDE_SNOOZE_MS = 60 * 60 * 1000;

/** what the panel shows: the overview, an agent session (by id), the transcriber or settings */
export type View = "home" | number | "transcribe" | "settings";

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
  /** the + menu in the tab row is open: the island makes room for it */
  const [tabMenu, setTabMenu] = useState(false);
  const [now, setNow] = useState(Date.now());

  const agents = useAgents(expanded, typeof view === "number" ? view : null);
  const review = useReview();
  const voice = useVoice();
  const gemini = useGemini();
  const media = useMedia();
  const [mascot] = useMascot();
  const rec = useTranscription(status, expanded && view === "transcribe");
  const { recording, paused, phase } = rec;
  const sessions = agents.sessions;
  const working = agents.running.length > 0;
  const reckless = agents.mode === "reckless";

  // ---- views ------------------------------------------------------------------
  const setView = (v: View) => {
    setViewState(v);
    // remembered by folder: session ids change between launches
    store.set("view", typeof v === "number" ? (agents.session(v)?.project ?? "") : v);
  };
  // first load, and when the shown session gets closed: fall back to something that exists
  useEffect(() => {
    if (!agents.status) return;
    if (view === "home" || view === "transcribe" || view === "settings" || (view != null && sessions.some((s) => s.id === view))) return;
    if (view == null) {
      const saved = store.get("view");
      if (saved === "transcribe" || saved === "home") return setViewState(saved);
      const match = sessions.find((s) => s.project === saved);
      if (match) return setViewState(match.id);
    }
    setViewState("home");
  }, [agents.status, view, sessions]);
  const current: SessionStatus | null = typeof view === "number" ? agents.session(view) : null;

  const openSession = async (path: string, provider?: Provider) => {
    const existing = sessions.find((s) => s.project.toLowerCase() === path.toLowerCase() && (!provider || s.provider === provider));
    if (existing) return setView(existing.id);
    const sid = await agents.open(path, provider);
    if (sid != null) {
      setViewState(sid);
      store.set("view", path);
    }
  };
  const [termError, setTermError] = useState<string | null>(null);
  const openTerminal = (path: string) => {
    setTermError(null);
    api.openTerminal(path).catch((e) => setTermError(String(e)));
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
  const compactSize = live ? COMPACT_LIVE : COMPACT;

  const claudeAlert = needsClaudeAttention(claude);
  const claudeBusy = claude?.state === "signing_in" || claude?.state === "installing";
  const showClaudeBanner = claudeBusy || (claudeAlert && Date.now() >= claudeSnoozedUntil);
  const showUpdate =
    update?.state === "downloading" ||
    ((update?.state === "available" || update?.state === "error") && update.available !== skippedBuild);
  const banners = (showClaudeBanner && claude ? 1 : 0) + (showUpdate && update ? 1 : 0);

  // the open notch is as tall as what it shows, like a chat that grows
  const viewH =
    view === "transcribe"
      ? recording
        ? 420
        : phase === "ready"
          ? Math.min(400, 170 + Math.max(1, rec.sources.length) * 46)
          : 230
      : view === "settings"
        ? SETTINGS_H
        : current
          ? current.running || agents.feed(current.id).length > 0
            ? 450
            : 340
          : CHROME_H + Math.max(132 + (media ? MEDIA_H : 0), Math.ceil(homePills(agents).count / 2) * PILL_ROW + 20);
  const expandedH = Math.min(EXPANDED_MAX_H, Math.max(viewH + banners * BANNER_H, tabMenu ? TAB_MENU_H : 0));
  const expandedHRef = useRef(expandedH);
  expandedHRef.current = expandedH;

  const clearTimers = () => {
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    collapseTimer.current = hoverTimer.current = null;
  };

  const expand = useCallback(async () => {
    clearTimers();
    await api.setNotchSize(EXPANDED_W, expandedHRef.current);
    setExpanded(true);
    api.claudeRefresh().catch(() => {});
  }, []);

  const collapse = useCallback(() => {
    clearTimers();
    setExpanded(false);
  }, []);

  // the hit area follows the open notch when the view grows or shrinks
  useEffect(() => {
    if (expanded) api.setNotchSize(EXPANDED_W, expandedH);
  }, [expanded, expandedH]);

  // the window never takes the keyboard by itself: the open notch borrows it, so
  // typing and shortcuts work without a click first, and gives it back on close
  useEffect(() => {
    api.notchFocus(expanded).catch(() => {});
  }, [expanded]);

  const keepOpen = pinned || hold || tabMenu || picking || rec.ending;
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
  const hitWidth = pills > 0 ? Math.max(compactSize.w, PILL_WIDTH) : compactSize.w;
  const hitHeight = compactSize.h + (pills > 0 ? 8 + pills * PILL_STEP : 0);
  useEffect(() => {
    if (!shell && !expanded) api.setNotchSize(hitWidth, hitHeight);
  }, [shell, expanded, hitWidth, hitHeight]);

  // ---- keyboard ---------------------------------------------------------------
  const order: View[] = ["home", "transcribe", ...sessions.map((s) => s.id)];
  const keys = useRef<(e: KeyboardEvent) => boolean>(() => false);
  keys.current = (e) => {
    const n = Number(e.key);
    const k = e.key.toLowerCase();
    if (e.ctrlKey && n >= 1 && n <= 9 && sessions[n - 1]) setView(sessions[n - 1].id);
    else if (e.ctrlKey && k === "h") setView("home");
    else if (e.ctrlKey && k === "m") setView("transcribe");
    else if (e.ctrlKey && e.key === ",") setView("settings");
    else if (e.ctrlKey && k === "t") pickAndOpen();
    else if (e.ctrlKey && e.code === "Backquote" && current) openTerminal(current.project);
    else if (e.ctrlKey && e.key === "Tab") {
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
  const size = expanded ? { w: EXPANDED_W, h: expandedH } : compactSize;
  const limit = agents.limit;
  const limitHit = limit?.status === "rejected" && (limit.resets_at ?? 0) > Date.now();
  // the session the collapsed bar talks about: the one you look at, else the first busy one
  const busy = agents.running.find((s) => s.id === view) ?? agents.running[0];
  const busyText = busy
    ? `${agents.running.length > 1 ? `${agents.running.length} working · ` : ""}${basename(busy.project)}: ${busy.current ?? "Working…"}`
    : "";
  const busyElapsed = busy?.started_at ? formatDuration(now - busy.started_at) : "";
  const busyStep = busy ? stepTime(busy, now) : "";
  const level = recording && !paused ? Math.max(rec.levels.app, rec.levels.mic) : 0;
  // when nothing is going on the bar holds just the orb and the sessions' colours;
  // only things that need you show up as text
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
  const barStatus: Status = voice.active
    ? "voice"
    : flash
      ? toneStatus(flash.tone)
      : recording
        ? paused
          ? "paused"
          : "rec"
        : working
          ? reckless
            ? "auto"
            : "work"
          : finished && notice
            ? toneStatus(notice.tone)
            : phase === "error" || limitHit
              ? "error"
              : claudeAlert
                ? "warn"
                : phase === "preparing"
                  ? "setup"
                  : "idle";
  const sessionGlow = (s: SessionStatus): Status => {
    const tone = agents.unseen[s.id];
    return s.running ? (reckless ? "auto" : "work") : tone ? toneStatus(tone) : "idle";
  };

  return (
    <div className="stage">
      <div
        className={[
          "island",
          expanded && "is-expanded",
          `status-${barStatus}`,
          finished && notice && "is-finished",
          igniting && "is-igniting",
        ]
          .filter(Boolean)
          .join(" ")}
        style={{ width: size.w, height: size.h }}
        onClick={() => !expanded && expand()}
        onPointerEnter={() => (overNotch.current = true)}
        onPointerLeave={() => (overNotch.current = false)}
      >
        {/* drops out from behind the collapsed notch while an agent works; review pills take its spot */}
        <NotchMascot id={mascot} out={!expanded && working && pills === 0} status={barStatus} />

        <div className="island-clip">
          {/* ---------- collapsed bar ---------- */}
          <div className="bar" onPointerEnter={() => !expanded && onEnter()}>
            <Orb status={barStatus} level={level} />
            {voice.active ? (
              <span className={`ticker ${voice.phase === "hearing" && voice.text ? "" : "muted"}`}>{voiceLine(voice)}</span>
            ) : flash ? (
              <>
                <span className={`notice-icon is-${flash.tone}`}>
                  {flash.tone === "ok" ? <CheckIcon /> : flash.tone === "warn" ? <AlertIcon /> : <CrossIcon />}
                </span>
                <span className="ticker">{flash.text}</span>
              </>
            ) : working && recording ? (
              <>
                <span className={`timer ${paused ? "is-paused" : ""}`}>{rec.elapsed}</span>
                <span className="ticker shimmer">{busyText}</span>
                {busyStep && <span className="bar-step-time">{busyStep}</span>}
              </>
            ) : working ? (
              <>
                <span className="ticker shimmer">{busyText}</span>
                {busyStep && <span className="bar-step-time">{busyStep}</span>}
                <span className="bar-time">{busyElapsed}</span>
              </>
            ) : recording ? (
              <>
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
                <Bars level={level} />
              </>
            ) : finished && notice ? (
              <>
                <span className="ticker">{notice.text}</span>
                <span className="bar-hint">hover to see</span>
              </>
            ) : (
              <>
                {idleMessage ? <span className="bar-label">{idleMessage}</span> : <span className="grow" />}
                {claudeAlert && (
                  <span className="claude-alert" title={claude?.message ?? "Claude needs attention"}>
                    <KeyIcon />
                  </span>
                )}
                {sessions.length > 0 && (
                  <span className="bar-minis">
                    {sessions.slice(0, 4).map((s) => (
                      <Mini key={s.id} hue={hue(s.project)} size={12} />
                    ))}
                  </span>
                )}
              </>
            )}
          </div>

          {/* ---------- expanded panel ---------- */}
          {shell && (
            <div className="panel" onClick={(e) => e.stopPropagation()}>
              <header className="header">
                <Tabs
                  agents={agents}
                  view={view}
                  recording={recording}
                  paused={paused}
                  elapsed={rec.elapsed}
                  picking={picking}
                  onView={setView}
                  onOpen={openSession}
                  onPick={pickAndOpen}
                  onHold={setTabMenu}
                />
                <div className="header-actions">
                  {claude?.state === "signed_in" && current?.provider !== "gemini" && <LimitChip plan={claude.plan} limit={agents.limit} usage={agents.usage} />}
                  {gemini?.state === "signed_in" && current?.provider === "gemini" && <GeminiLimitChip usage={gemini.usage} model={agents.geminiModel} />}
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
                  <IconButton title="Settings: accounts, default agent (Ctrl+,)" active={view === "settings"} onClick={() => setView(view === "settings" ? "home" : "settings")}>
                    <GearIcon />
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

              <div className="view" key={String(view)}>
                {view === "settings" ? (
                  <Settings claude={claude} gemini={gemini} agents={agents} voice={voice} />
                ) : view === "transcribe" ? (
                  <section className={`card wash glow-${recording ? (paused ? "paused" : "rec") : phase === "error" ? "error" : "idle"}`}>
                    <TranscribeTab t={rec} status={status} />
                  </section>
                ) : current ? (
                  <section className={`card wash glow-${sessionGlow(current)}`} style={{ "--h": hue(current.project) } as CSSProperties}>
                    <header className="card-head">
                      <ProjectMenu agents={agents} session={current} onPick={pickAndOpen} onHold={setHold} onTerminal={openTerminal} />
                      <button
                        className="btn secondary sm"
                        title={"Open a terminal in this folder (Ctrl+`)\nClaude can open one too, for dev servers and watchers"}
                        onClick={() => openTerminal(current.project)}
                      >
                        <TerminalIcon /> Terminal
                      </button>
                      <button
                        className="btn secondary sm"
                        title="New conversation (Ctrl+N)"
                        disabled={current.running || agents.feed(current.id).length === 0}
                        onClick={() => agents.newChat(current.id)}
                      >
                        <PlusIcon /> New chat
                      </button>
                    </header>
                    {termError && <p className="error-text">{termError}</p>}
                    <CodeTab key={current.id} agents={agents} session={current} voice={voice} gemini={gemini} onHold={setHold} />
                  </section>
                ) : (
                  <Home
                    rec={rec}
                    status={status}
                    agents={agents}
                    now={now}
                    picking={picking}
                    onView={setView}
                    onOpen={openSession}
                    onPick={pickAndOpen}
                    onHold={setHold}
                    media={media}
                  />
                )}
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

/**
 * The top of the panel: the overview, the transcriber, one tab per Claude
 * session in its own colour (its state at a glance), and + for another folder.
 */
function Tabs({
  agents,
  view,
  recording,
  paused,
  elapsed,
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
  elapsed: string;
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
    <nav className="tabs">
      <button className={`tab tab-icon ${view === "home" ? "is-active" : ""}`} title="Overview (Ctrl+H)" onClick={() => onView("home")}>
        <HomeIcon />
      </button>

      <button
        className={`tab tab-icon tab-rec ${view === "transcribe" ? "is-active" : ""} ${recording ? (paused ? "is-paused" : "is-recording") : ""}`}
        title="Transcribe (Ctrl+M)"
        onClick={() => onView("transcribe")}
      >
        {recording ? <span className="tab-rec-dot" /> : <MicIcon />}
        {recording && <span className="tab-timer">{elapsed}</span>}
      </button>

      <span className="tabs-sep" />

      {agents.sessions.map((s, i) => {
        const tone = agents.unseen[s.id];
        const queued = agents.queue(s.id).length;
        return (
          <div key={s.id} className="tab-slot">
            <button
              className={`tab tab-session ${view === s.id ? "is-active" : ""} ${s.running ? "is-running" : ""}`}
              style={{ "--h": hue(s.project) } as CSSProperties}
              title={`${basename(s.project)} · ${PROVIDER_NAME[s.provider]}${s.running ? ` — ${s.current ?? "working"}` : ""}\n${s.project}\nCtrl+${i + 1}`}
              onClick={() => onView(s.id)}
            >
              <Mini hue={hue(s.project)} running={s.running} size={10} />
              <span className="tab-name">{basename(s.project)}</span>
              <AgentTag provider={s.provider} />
              {tone && !s.running && <span className={`tab-badge is-${tone}`} />}
              {queued > 0 && <span className="tab-count">{queued}</span>}
            </button>
            <button
              className="tab-x"
              title={s.running ? `Stop ${PROVIDER_NAME[s.provider]} and close this session` : "Close this session"}
              onClick={() => agents.close(s.id)}
            >
              <CrossIcon />
            </button>
          </div>
        );
      })}

      <div className="tab-add">
        <button
          className={`tab tab-plus ${adding ? "is-active" : ""}`}
          title={`New session in a folder (Ctrl+T)`}
          disabled={picking || agents.sessions.length >= 9}
          onClick={() => setAdding(!adding)}
        >
          <PlusIcon />
          {agents.sessions.length === 0 && PROVIDER_NAME[agents.defaultProvider]}
        </button>
        {adding && (
          <>
            <div className="menu-scrim" onClick={() => setAdding(false)} />
            <div className="menu menu-down" role="menu">
              <div className="menu-agent" role="radiogroup" aria-label="Agent for the new session">
                <span className="menu-agent-label">Agent</span>
                <div className="seg">
                  {(["claude", "gemini"] as const).map((p) => (
                    <button
                      key={p}
                      type="button"
                      role="radio"
                      aria-checked={agents.defaultProvider === p}
                      className={agents.defaultProvider === p ? "on" : ""}
                      onClick={() => agents.setDefaultProvider(p)}
                    >
                      <AgentTag provider={p} /> {PROVIDER_NAME[p]}
                    </button>
                  ))}
                </div>
              </div>
              <div className="menu-sep" />
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
              {recent.length > 0 && <div className="menu-sep" />}
              <button
                className="menu-item"
                role="menuitem"
                onClick={() => {
                  setAdding(false);
                  onPick();
                }}
              >
                <span className="menu-title">{recent.length ? "Choose another folder…" : "Choose a folder…"}</span>
              </button>
            </div>
          </>
        )}
      </div>
    </nav>
  );
}

/** The session's folder: open it (Explorer, terminal), move the session, start another, close it. */
function ProjectMenu({
  agents,
  session,
  onPick,
  onHold,
  onTerminal,
}: {
  agents: Agents;
  session: SessionStatus;
  onPick: () => void;
  onHold: (hold: boolean) => void;
  onTerminal: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  useEffect(() => onHold(open), [open, onHold]);
  const others = agents.recent.filter((p) => p.toLowerCase() !== session.project.toLowerCase());
  const other: Provider = session.provider === "gemini" ? "claude" : "gemini";
  const queued = agents.queue(session.id).length;
  return (
    <div className="project">
      <button className={`project-btn ${open ? "is-open" : ""}`} title={session.project} onClick={() => setOpen(!open)}>
        <Mini hue={hue(session.project)} running={session.running} size={10} />
        <span className="project-name">{basename(session.project)}</span>
        <ChevronIcon open={open} />
      </button>
      <span className="session-label">
        {PROVIDER_NAME[session.provider]} ·{" "}
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
            <button
              className="menu-item"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                onTerminal(session.project);
              }}
            >
              <span className="menu-title">Open terminal here (Ctrl+`)</span>
            </button>
            {!session.running && (
              <>
                <div className="menu-sep" />
                <button
                  className="menu-item"
                  role="menuitem"
                  onClick={() => {
                    setOpen(false);
                    agents.setProvider(session.id, other);
                  }}
                >
                  <span className="menu-title">Switch this session to {PROVIDER_NAME[other]}</span>
                  <span className="menu-sub">Starts a new conversation · the other agent's sessions keep working</span>
                </button>
              </>
            )}
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

/**
 * How much of Gemini's plan is used, from Antigravity's `/usage`: the limit of the
 * chosen model's group (Gemini, or Claude and GPT) in the chip; a click shows them all.
 */
function GeminiLimitChip({ usage, model }: { usage: GeminiUsage | null; model: string }) {
  const [open, setOpen] = useState(false);
  const quotas = usage?.quotas ?? [];
  const thirdParty = /^(claude|gpt)/i.test(model);
  const quota = quotas.find((q) => /gemini/i.test(q.group) !== thirdParty) ?? quotas[0];
  if (!quota) {
    return (
      <span className="chip limit-chip is-gemini" title="Gemini plan usage shows up once Antigravity CLI reports it">
        Gemini
      </span>
    );
  }
  const pct = Math.min(100, Math.round(quota.used * 100));
  const tone = pct >= 100 ? "is-error" : pct >= 80 ? "is-warn" : "";
  const span = quota.window === "weekly" ? "week" : quota.window === "daily" ? "day" : null;
  const reset = (ms: number) =>
    new Date(ms).toLocaleString([], { weekday: ms - Date.now() > 20 * 3_600_000 ? "short" : undefined, hour: "2-digit", minute: "2-digit" });
  const title = quotas
    .map((q) => `${q.group}: ${Math.round(q.used * 100)}% of the ${q.window ? `${q.window} ` : ""}limit used${q.resets_at ? `, refills ${reset(q.resets_at)}` : ""}`)
    .join("\n");
  return (
    <span className="limit-wrap">
      <button type="button" className={`chip limit-chip is-gemini ${tone} ${open ? "open" : ""}`} title={`${title}\nClick for details`} onClick={() => setOpen(!open)}>
        {span}
        <span className="limit-meter">
          <i style={{ width: `${pct}%` }} />
        </span>
        {pct >= 100 ? "limit reached" : `${pct}% used`}
      </button>
      {open && (
        <>
          <div className="menu-scrim" onClick={() => setOpen(false)} />
          <div className="usage-pop">
            <UsageCard report={geminiReport(quotas)} />
          </div>
        </>
      )}
    </span>
  );
}

/** Plan + how much of it is used: Claude Code's `/usage`, else what Claude's replies reported. */
function LimitChip({ plan, limit, usage }: { plan: string | null; limit: Limit | null; usage: Usage | null }) {
  const current = !!limit && ((limit.resets_at ?? 0) > Date.now() || limit.status === "rejected");
  const rejected = current && limit!.status === "rejected";
  const used = usage?.session ?? (current ? limit!.used : null);
  const pct = used != null ? Math.min(100, Math.round(used * 100)) : null;
  const tone = rejected || (pct ?? 0) >= 100 ? "is-error" : (current && limit!.status === "allowed_warning") || (pct ?? 0) >= 80 ? "is-warn" : "";
  const reset = current && limit!.resets_at ? clock(limit!.resets_at) : null;
  const title = [
    `Claude${plan ? ` ${capitalize(plan)}` : ""}`,
    pct != null ? `5-hour session: ${pct}% used` : rejected ? "5-hour limit reached" : "usage shown after Claude's next reply",
    usage?.week != null ? `week: ${Math.round(usage.week * 100)}% used` : null,
    reset ? `resets at ${reset}` : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <span className={`chip limit-chip ${tone}`} title={title}>
      {pct != null ? (
        <>
          5h
          <span className="limit-meter">
            <i style={{ width: `${pct}%` }} />
          </span>
          {pct}% used
        </>
      ) : rejected ? (
        <>limit reached</>
      ) : (
        <>Claude{plan ? ` · ${capitalize(plan)}` : ""}</>
      )}
    </span>
  );
}
