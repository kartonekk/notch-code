import { useCallback, useEffect, useRef, useState } from "react";
import { api, AppStatus, ClaudeStatus, Limit, on, UpdateStatus } from "./api";
import { CodeTab, useAgent } from "./CodeTab";
import { RecklessRoad } from "./Reckless";
import { ReviewPill, useReview } from "./Review";
import { SPEAKER_LABEL, TranscribeTab, useTranscription } from "./TranscribeTab";
import {
  AlertIcon,
  Bars,
  capitalize,
  CheckIcon,
  ClaudeBanner,
  CodeIcon,
  CrossIcon,
  FolderIcon,
  formatDuration,
  IconButton,
  KeyIcon,
  MicIcon,
  needsClaudeAttention,
  PinIcon,
  PowerIcon,
  StatusDot,
  store,
  UpdateBanner,
} from "./ui";
import "./notch.css";

// Notch sizes in logical pixels. The window itself always has the EXPANDED size
// (see WINDOW_W in lib.rs); the backend makes everything outside the current
// notch click-through and reports hover via the `notch-hover` event.
const COLLAPSED = { w: 280, h: 36 };
const COLLAPSED_LIVE = { w: 460, h: 36 };
const COLLAPSED_RECKLESS = { w: 400, h: 36 };
/** room under the collapsed notch for the accept / reject pill */
const PILL_SPACE = 50;
const EXPANDED = { w: 620, h: 480 };
const ANIM_MS = 260;
/** "Later" on the Claude sign-in reminder hides it for this long */
const CLAUDE_SNOOZE_MS = 60 * 60 * 1000;

type Tab = "code" | "transcribe";

export default function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [claude, setClaude] = useState<ClaudeStatus | null>(null);
  const [claudeSnoozedUntil, setClaudeSnoozedUntil] = useState(0);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  /** build the user said "Later" to; a newer build shows the notice again */
  const [skippedBuild, setSkippedBuild] = useState<number | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [tab, setTabState] = useState<Tab>(() => (store.get("tab") === "transcribe" ? "transcribe" : "code"));
  /** a tab is in the middle of something (typing, choosing a folder) */
  const [hold, setHold] = useState(false);
  const [now, setNow] = useState(Date.now());

  const setTab = useCallback((t: Tab) => {
    setTabState(t);
    store.set("tab", t);
  }, []);

  const agent = useAgent(expanded);
  const review = useReview();
  const rec = useTranscription(status, expanded && tab === "transcribe");
  const { recording, paused, phase } = rec;
  const working = agent.running;
  const reckless = agent.mode === "reckless";

  // a short shake + flash when reckless mode is switched on
  const [igniting, setIgniting] = useState(false);
  const wasReckless = useRef(reckless);
  useEffect(() => {
    if (reckless && !wasReckless.current) {
      setIgniting(true);
      const t = window.setTimeout(() => setIgniting(false), 700);
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
  /** the pointer is on the review pill: hovering there must not open the notch */
  const overPill = useRef(false);
  const pill = !expanded && review.files.length > 0 && !working && !reckless;
  const live = recording || working || !!agent.notice || pill;
  const collapsedSize = live ? COLLAPSED_LIVE : reckless ? COLLAPSED_RECKLESS : COLLAPSED;

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

  const keepOpen = pinned || hold || rec.ending;
  const onEnter = () => {
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    collapseTimer.current = null;
    if (!expanded && !hoverTimer.current)
      hoverTimer.current = window.setTimeout(() => {
        hoverTimer.current = null;
        if (!overPill.current) expand();
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
  const hitHeight = collapsedSize.h + (pill ? PILL_SPACE : 0);
  useEffect(() => {
    if (!shell && !expanded) api.setNotchSize(collapsedSize.w, hitHeight);
  }, [shell, expanded, collapsedSize.w, hitHeight]);

  // ---- keyboard ---------------------------------------------------------------
  useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "1") setTab("code");
      else if (e.ctrlKey && e.key === "2") setTab("transcribe");
      else if (e.key === "Escape" && !pinned && !rec.ending) collapse();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [expanded, pinned, rec.ending, collapse, setTab]);

  // ---- derived ------------------------------------------------------------------
  const size = expanded ? EXPANDED : collapsedSize;
  const claudeAlert = needsClaudeAttention(claude);
  const claudeBusy = claude?.state === "signing_in" || claude?.state === "installing";
  const showClaudeBanner = claudeBusy || (claudeAlert && Date.now() >= claudeSnoozedUntil);
  const showUpdate =
    update?.state === "downloading" ||
    ((update?.state === "available" || update?.state === "error") && update.available !== skippedBuild);
  const agentElapsed = agent.status?.started_at ? formatDuration(now - agent.status.started_at) : "";
  const limit = agent.limit;
  const limitHit = limit?.status === "rejected" && (limit.resets_at ?? 0) > Date.now();
  // when nothing is going on the notch stays empty (room for later); only things that need you show up
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
        className={`notch ${expanded ? "is-expanded" : ""} phase-${phase} ${reckless ? "is-reckless" : ""} ${working ? "is-working" : ""} ${igniting ? "is-igniting" : ""}`}
        style={{ width: size.w, height: size.h }}
        onClick={() => !expanded && expand()}
      >
        <span className="ear ear-l" />
        <span className="ear ear-r" />
        <div className="notch-clip">
          {/* ---------- collapsed bar ---------- */}
          <div className="bar" onPointerEnter={() => !expanded && onEnter()}>
            {reckless && !recording ? (
              <RecklessRoad text={working ? (agent.status?.current ?? "Working…") : "Reckless driving"} fast={working} />
            ) : working && recording ? (
              <>
                <StatusDot phase={paused ? "paused" : "recording"} />
                <span className={`timer ${paused ? "is-paused" : ""}`}>{rec.elapsed}</span>
                <span className="mini-spinner agent-spin" />
                <span className="ticker">{agent.status?.current ?? "Working…"}</span>
              </>
            ) : working ? (
              <>
                <span className="mini-spinner agent-spin" />
                <span className="ticker">{agent.status?.current ?? "Working…"}</span>
                <span className="bar-time">{agentElapsed}</span>
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
            ) : agent.notice ? (
              <>
                <span className={`notice-icon is-${agent.notice.tone}`}>
                  {agent.notice.tone === "ok" ? <CheckIcon /> : agent.notice.tone === "warn" ? <AlertIcon /> : <CrossIcon />}
                </span>
                <span className="ticker">{agent.notice.text}</span>
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

          {pill && <ReviewPill review={review} onOpen={() => { setTab("code"); expand(); }} onHover={(over) => (overPill.current = over)} />}

          {/* ---------- expanded panel ---------- */}
          {shell && (
            <div className="panel" onClick={(e) => e.stopPropagation()}>
              <header className="panel-head">
                <div className="tabs" role="tablist">
                  <button
                    role="tab"
                    aria-selected={tab === "code"}
                    className={`tab ${tab === "code" ? "is-active" : ""}`}
                    title="Code (Ctrl+1)"
                    onClick={() => setTab("code")}
                  >
                    <CodeIcon /> Code
                    {working && <span className="mini-spinner" />}
                  </button>
                  <button
                    role="tab"
                    aria-selected={tab === "transcribe"}
                    className={`tab ${tab === "transcribe" ? "is-active" : ""}`}
                    title="Transcribe (Ctrl+2)"
                    onClick={() => setTab("transcribe")}
                  >
                    <MicIcon /> Transcribe
                    {recording && <span className={`tab-badge ${paused ? "is-paused" : ""}`}>{paused ? "Paused" : rec.elapsed}</span>}
                  </button>
                </div>
                <div className="head-actions">
                  {tab === "code" && claude?.state === "signed_in" && <LimitChip plan={claude.plan} limit={agent.limit} />}
                  {tab === "transcribe" && status?.backend && <span className="chip">{status.backend}</span>}
                  <IconButton title={pinned ? "Unpin" : "Keep open"} active={pinned} onClick={() => setPinned(!pinned)}>
                    <PinIcon />
                  </IconButton>
                  <IconButton
                    title={tab === "code" ? "Open the project folder" : "Transcripts folder"}
                    disabled={tab === "code" && !agent.status?.project}
                    onClick={() => (tab === "code" ? api.agentOpenProject() : api.openFolder()).catch(() => {})}
                  >
                    <FolderIcon />
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
                {tab === "code" ? <CodeTab agent={agent} review={review} onHold={setHold} /> : <TranscribeTab t={rec} status={status} />}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** "14:30" */
function clock(ms: number) {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
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
