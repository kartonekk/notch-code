import { useMemo } from "react";
import type { CSSProperties, ReactNode } from "react";
import { api, AppStatus, SessionStatus } from "./api";
import type { View } from "./App";
import type { Agents, FeedItem } from "./CodeTab";
import { SPEAKER_LABEL, Transcription } from "./TranscribeTab";
import { basename, formatDuration, hue, MicIcon, Mini, PauseIcon, PlayIcon, PlusIcon, Status, StepGlyph } from "./ui";

/** what the pill grid on the right holds: the transcriber, every session, a few recent folders, + */
export function homePills(agents: Agents) {
  const openPaths = new Set(agents.sessions.map((s) => s.project.toLowerCase()));
  const recent = agents.recent.filter((p) => !openPaths.has(p.toLowerCase())).slice(0, Math.max(0, 4 - agents.sessions.length));
  return { recent, count: 1 + agents.sessions.length + recent.length + (agents.sessions.length < 9 ? 1 : 0) };
}

/**
 * The overview, one look at everything. On the left, a card says what's going
 * on right now and offers the next step; on the right, one pill per thing you
 * can jump into (transcriber, each Claude session, recent folders).
 */
export function Home({
  rec,
  status,
  agents,
  now,
  picking,
  onView,
  onOpen,
  onPick,
}: {
  rec: Transcription;
  status: AppStatus | null;
  agents: Agents;
  now: number;
  picking: boolean;
  onView: (v: View) => void;
  onOpen: (path: string) => void;
  onPick: () => void;
}) {
  const { recent } = homePills(agents);
  return (
    <div className="overview">
      <Hero rec={rec} status={status} agents={agents} now={now} onView={onView} onPick={onPick} />

      <section className="card pills-card">
        <div className="pills">
          <Pill
            mini={
              <span className={`mini mini-rec ${rec.recording ? (rec.paused ? "is-paused" : "is-live") : ""}`}>
                <MicIcon />
              </span>
            }
            label={rec.recording ? rec.elapsed : "Transcribe"}
            title={
              rec.recording
                ? `${rec.paused ? "Paused" : "Recording"} · ${rec.elapsed}${status?.source ? ` · ${status.source}` : ""} (Ctrl+M)`
                : rec.phase === "preparing"
                  ? "Getting the speech model ready…"
                  : rec.phase === "error"
                    ? "Speech model failed — open to retry"
                    : "Record a meeting from any app (Ctrl+M)"
            }
            tone={rec.recording ? (rec.paused ? "warn" : "rec") : undefined}
            onClick={() => onView("transcribe")}
          />
          {agents.sessions.map((s, i) => (
            <SessionPill key={s.id} s={s} agents={agents} now={now} index={i} onClick={() => onView(s.id)} />
          ))}
          {recent.map((p) => (
            <Pill key={p} ghost mini={<Mini hue={hue(p)} />} label={basename(p)} title={`Open a Claude session in ${p}`} onClick={() => onOpen(p)} />
          ))}
          {agents.sessions.length < 9 && (
            <Pill
              ghost
              mini={
                <span className="mini mini-plus">
                  <PlusIcon />
                </span>
              }
              label={picking ? "Choosing…" : "New session"}
              title="Pick the folder Claude works in (Ctrl+T)"
              onClick={onPick}
            />
          )}
        </div>
        {agents.error && <p className="error-text pills-error">{agents.error}</p>}
      </section>
    </div>
  );
}

/** the last few steps of a session's current run, newest last */
function recentSteps(feed: FeedItem[], n: number) {
  const steps: Extract<FeedItem, { type: "step" }>[] = [];
  for (let i = feed.length - 1; i >= 0 && steps.length < n; i--) {
    const it = feed[i];
    if (it.type === "prompt") break;
    if (it.type === "step") steps.unshift(it);
  }
  return steps;
}

/** What's happening now, biggest thing first, with its next step. */
function Hero({
  rec,
  status,
  agents,
  now,
  onView,
  onPick,
}: {
  rec: Transcription;
  status: AppStatus | null;
  agents: Agents;
  now: number;
  onView: (v: View) => void;
  onPick: () => void;
}) {
  const busy = agents.running[0];
  const busyFeed = busy ? agents.feed(busy.id) : [];
  const steps = useMemo(() => recentSteps(busyFeed, 2), [busyFeed]);
  let glow: Status;
  let who: ReactNode;
  let body: ReactNode;
  let actions: ReactNode = null;

  if (rec.recording) {
    glow = rec.paused ? "paused" : "rec";
    who = (
      <>
        <span className={`dot ${rec.paused ? "is-paused" : "is-rec"}`} />
        <b className="name">{rec.paused ? "Paused" : "Recording"}</b>
        <span className="tool">{status?.source ?? "the meeting"}</span>
        <span className={`count timer ${rec.paused ? "is-paused" : ""}`}>{rec.elapsed}</span>
      </>
    );
    body = (
      <p className="hero-line">
        {rec.paused ? (
          <span className="muted">Nothing is written while paused</span>
        ) : rec.lastLine ? (
          <>
            <b>{SPEAKER_LABEL[rec.lastLine.speaker]}:</b> {rec.lastLine.text}
          </>
        ) : (
          <span className="muted">Listening…</span>
        )}
      </p>
    );
    actions = (
      <>
        <button className="btn secondary" onClick={rec.togglePause}>
          {rec.paused ? <PlayIcon /> : <PauseIcon />} {rec.paused ? "Resume" : "Pause"}
        </button>
        <button className="btn secondary" onClick={() => onView("transcribe")}>
          Transcript
        </button>
        <button
          className="btn primary"
          onClick={() => {
            rec.setEnding(true);
            onView("transcribe");
          }}
        >
          <span className="stop-sq" /> End
        </button>
      </>
    );
  } else if (busy) {
    glow = agents.mode === "reckless" ? "auto" : "work";
    who = (
      <>
        <span className="dot" style={{ "--h": hue(busy.project) } as CSSProperties} />
        <b className="name">{basename(busy.project)}</b>
        <span className="tool">Claude Code{agents.mode === "reckless" ? " · Auto" : ""}</span>
        <span className="count">
          {agents.running.length > 1 ? `${agents.running.length} working · ` : ""}
          {busy.started_at ? formatDuration(now - busy.started_at) : ""}
        </span>
      </>
    );
    body = (
      <div className="ticker">
        {steps.slice(0, -1).map((s) => (
          <div key={s.id} className="ticker-row is-past">
            <span className="tick-icon">
              <StepGlyph icon={s.icon} />
            </span>
            <span className="tick-text">{s.label}</span>
          </div>
        ))}
        <div className="ticker-row">
          <span className="tick-icon">
            <span className="mini-spinner" />
          </span>
          <span className="tick-text shimmer">{busy.current ?? "Thinking…"}</span>
        </div>
      </div>
    );
    actions = (
      <>
        <button className="btn primary" onClick={() => onView(busy.id)}>
          Watch
        </button>
        <button className="btn secondary" onClick={() => agents.stop(busy.id)}>
          <span className="stop-sq" /> Stop
        </button>
      </>
    );
  } else if (rec.phase === "preparing") {
    const p = status?.progress;
    glow = "setup";
    who = (
      <>
        <span className="dot is-setup" />
        <b className="name">Getting ready</b>
        <span className="tool">speech model</span>
        {p != null && <span className="count">{Math.round(p * 100)}%</span>}
      </>
    );
    body = (
      <>
        <p className="hero-line muted">{status?.message ?? "Preparing the speech model…"}</p>
        {p != null && (
          <span className="progress">
            <span className="progress-fill" style={{ width: `${Math.round(p * 100)}%` }} />
          </span>
        )}
      </>
    );
  } else if (rec.phase === "error") {
    glow = "error";
    who = (
      <>
        <span className="dot is-error" />
        <b className="name">Speech model failed</b>
      </>
    );
    body = <p className="hero-line detail">{status?.message}</p>;
    actions = (
      <button className="btn primary" onClick={() => api.retrySetup()}>
        Try again
      </button>
    );
  } else {
    const target = agents.sessions[0];
    glow = "idle";
    who = (
      <>
        <span className="dot is-idle" />
        <b className="name">Notch Code</b>
        <span className="tool">ready</span>
      </>
    );
    body = <p className="hero-line muted">Record a meeting, or ask Claude to do something.</p>;
    actions = (
      <>
        <button className="btn primary" onClick={() => onView("transcribe")}>
          <span className="rec-dot" /> Transcribe
        </button>
        <button className="btn secondary" onClick={() => (target ? onView(target.id) : onPick())}>
          Ask Claude
        </button>
      </>
    );
  }

  return (
    <section className={`card wash glow-${glow} hero`}>
      <div className="who">{who}</div>
      <div className="hero-body">{body}</div>
      {actions && <div className="actions">{actions}</div>}
    </section>
  );
}

function SessionPill({ s, agents, now, index, onClick }: { s: SessionStatus; agents: Agents; now: number; index: number; onClick: () => void }) {
  const tone = agents.unseen[s.id];
  const queued = agents.queue(s.id).length;
  const sub = s.running
    ? `${s.current ?? "Thinking…"}${s.started_at ? ` · ${formatDuration(now - s.started_at)}` : ""}`
    : tone === "ok"
      ? "Done — new reply to read"
      : tone
        ? "Stopped — needs a look"
        : s.has_session
          ? "Conversation"
          : "New conversation";
  return (
    <Pill
      mini={<Mini hue={hue(s.project)} running={s.running} />}
      label={basename(s.project)}
      title={`${basename(s.project)} — ${sub}${queued ? ` · ${queued} queued` : ""}${index < 9 ? ` (Ctrl+${index + 1})` : ""}\n${s.project}`}
      tone={s.running ? "busy" : tone}
      badge={queued || undefined}
      tinted
      style={{ "--h": hue(s.project) } as CSSProperties}
      onClick={onClick}
    />
  );
}

function Pill({
  mini,
  label,
  title,
  tone,
  badge,
  ghost,
  tinted,
  style,
  onClick,
}: {
  mini: ReactNode;
  label: string;
  title: string;
  tone?: "rec" | "busy" | "ok" | "warn" | "error";
  badge?: number;
  ghost?: boolean;
  tinted?: boolean;
  style?: CSSProperties;
  onClick: () => void;
}) {
  return (
    <button
      className={`pill ${ghost ? "is-ghost" : ""} ${tinted ? "is-tinted" : ""} ${tone ? `is-${tone}` : ""}`}
      style={style}
      title={title}
      onClick={onClick}
    >
      {mini}
      <span className="lbl">{label}</span>
      {tone && tone !== "busy" && tone !== "rec" && <span className={`pill-badge is-${tone}`} />}
      {badge != null && <span className="pill-count">{badge}</span>}
    </button>
  );
}
