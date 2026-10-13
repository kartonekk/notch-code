import { useEffect, useRef, useState, type CSSProperties, type MouseEvent, type ReactNode } from "react";
import { api, AppStatus, SessionStatus } from "./api";
import type { View } from "./App";
import type { Agents } from "./CodeTab";
import { Mood, Hum, toneMood } from "./Hum";
import { SPEAKER_LABEL, Transcription } from "./TranscribeTab";
import { basename, formatDuration, PauseIcon, PlayIcon, PlusIcon } from "./ui";

/** a stable colour per folder */
export function hue(path: string) {
  let h = 0;
  for (const c of path.toLowerCase()) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

/**
 * The overview, one look at everything: Hum on the left says what's going on
 * right now and offers the next step; on the right, one tile per thing you can
 * jump into (transcriber, each Claude session, recent folders).
 */
export function Home({
  mood,
  level,
  rec,
  status,
  agents,
  now,
  picking,
  onView,
  onOpen,
  onPick,
  onHold,
}: {
  mood: Mood;
  level: number;
  rec: Transcription;
  status: AppStatus | null;
  agents: Agents;
  now: number;
  picking: boolean;
  onView: (v: View) => void;
  onOpen: (path: string) => void;
  onPick: () => void;
  onHold: (hold: boolean) => void;
}) {
  const openPaths = new Set(agents.sessions.map((s) => s.project.toLowerCase()));
  const recent = agents.recent.filter((p) => !openPaths.has(p.toLowerCase())).slice(0, Math.max(0, 4 - agents.sessions.length));
  const homeRef = useRef<HTMLDivElement>(null);
  /** right-click menu on a folder tile, placed relative to .home */
  const [menu, setMenu] = useState<{ path: string; x: number; y: number } | null>(null);
  useEffect(() => onHold(menu != null), [menu, onHold]);
  useEffect(() => () => onHold(false), [onHold]);
  const openMenu = (e: MouseEvent, path: string) => {
    e.preventDefault();
    const box = homeRef.current?.getBoundingClientRect();
    if (!box) return;
    const w = 220;
    const h = 90;
    setMenu({
      path,
      x: Math.max(0, Math.min(e.clientX - box.left, box.width - w)),
      y: Math.max(0, Math.min(e.clientY - box.top, box.height - h)),
    });
  };
  return (
    <div className="home" ref={homeRef}>
      <section className="card hero">
        <Hum mood={mood} size={150} level={level} look className="hero-hum" />
        <Hero rec={rec} status={status} agents={agents} now={now} onView={onView} onPick={onPick} />
      </section>

      <section className="card tiles">
        <Tile
          hum={<Hum mood={rec.recording ? (rec.paused ? "paused" : "listen") : rec.phase === "preparing" ? "work" : rec.phase === "error" ? "error" : "idle"} level={level} size={44} />}
          title="Transcribe"
          sub={
            rec.recording
              ? `${rec.paused ? "Paused" : "Recording"} · ${rec.elapsed}${status?.source ? ` · ${status.source}` : ""}`
              : rec.phase === "preparing"
                ? "Getting the speech model ready…"
                : rec.phase === "error"
                  ? "Speech model failed — open to retry"
                  : "Record a meeting from any app"
          }
          hint="Ctrl+M"
          tone={rec.recording && !rec.paused ? "rec" : undefined}
          onClick={() => onView("transcribe")}
        />
        {agents.sessions.map((s, i) => (
          <SessionTile key={s.id} s={s} agents={agents} now={now} index={i} onClick={() => onView(s.id)} />
        ))}
        {recent.map((p) => (
          <Tile
            key={p}
            ghost
            hum={<Hum mood="sleep" hue={hue(p)} size={44} />}
            title={basename(p)}
            sub="Open a Claude session here"
            onClick={() => onOpen(p)}
            onContextMenu={(e) => openMenu(e, p)}
          />
        ))}
        {agents.sessions.length < 9 && (
          <Tile
            ghost
            hum={
              <span className="tile-plus">
                <PlusIcon />
              </span>
            }
            title={picking ? "Choosing…" : "New Claude session"}
            sub="Pick the folder Claude works in"
            hint="Ctrl+T"
            onClick={onPick}
          />
        )}
        {agents.error && <p className="error-text tiles-error">{agents.error}</p>}
      </section>

      {menu && (
        <>
          <div
            className="menu-scrim"
            onClick={() => setMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu(null);
            }}
          />
          <div className="menu menu-context" role="menu" style={{ left: menu.x, top: menu.y }}>
            <button
              className="menu-item"
              role="menuitem"
              onClick={() => {
                setMenu(null);
                onOpen(menu.path);
              }}
            >
              <span className="menu-title">Open a Claude session here</span>
              <span className="menu-sub">{menu.path}</span>
            </button>
            <div className="menu-sep" />
            <button
              className="menu-item is-danger"
              role="menuitem"
              onClick={() => {
                setMenu(null);
                api.agentForgetFolder(menu.path).catch(() => {});
              }}
            >
              <span className="menu-title">Remove from Home</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
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
  let title: ReactNode;
  let sub: ReactNode;
  let actions: ReactNode;

  if (rec.recording) {
    title = (
      <>
        <span className={`timer ${rec.paused ? "is-paused" : ""}`}>{rec.elapsed}</span> {rec.paused ? "Paused" : "Recording"}
      </>
    );
    sub = rec.paused ? (
      <span className="muted">Nothing is written while paused</span>
    ) : rec.lastLine ? (
      <>
        <b>{SPEAKER_LABEL[rec.lastLine.speaker]}:</b> {rec.lastLine.text}
      </>
    ) : (
      <span className="muted">Listening to {status?.source ?? "the meeting"}…</span>
    );
    actions = (
      <>
        <button className="btn" onClick={rec.togglePause}>
          {rec.paused ? <PlayIcon /> : <PauseIcon />} {rec.paused ? "Resume" : "Pause"}
        </button>
        <button className="btn btn-ghost" onClick={() => onView("transcribe")}>
          Transcript
        </button>
        <button
          className="btn btn-stop"
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
    title = agents.running.length > 1 ? `Claude is working in ${agents.running.length} folders` : `Claude is working`;
    sub = (
      <>
        <b>{basename(busy.project)}:</b> {busy.current ?? "Thinking…"}
        {busy.started_at && <span className="muted"> · {formatDuration(now - busy.started_at)}</span>}
      </>
    );
    actions = (
      <>
        <button className="btn btn-accent" onClick={() => onView(busy.id)}>
          Watch
        </button>
        <button className="btn btn-ghost" onClick={() => agents.stop(busy.id)}>
          <span className="stop-sq" /> Stop
        </button>
      </>
    );
  } else if (rec.phase === "preparing") {
    const p = status?.progress;
    title = "Getting ready";
    sub = (
      <>
        <span className="muted">{status?.message ?? "Preparing the speech model…"}</span>
        {p != null && (
          <span className="progress hero-progress">
            <span className="progress-fill" style={{ width: `${Math.round(p * 100)}%` }} />
          </span>
        )}
      </>
    );
  } else if (rec.phase === "error") {
    title = "Speech model failed";
    sub = <span className="muted">{status?.message}</span>;
    actions = (
      <button className="btn btn-accent" onClick={() => api.retrySetup()}>
        Try again
      </button>
    );
  } else {
    const target = agents.sessions[0];
    title = "Hi! What's next?";
    sub = <span className="muted">Record a meeting, or ask Claude to do something.</span>;
    actions = (
      <>
        <button className="btn btn-record" onClick={() => onView("transcribe")}>
          <span className="rec-dot" /> Transcribe
        </button>
        <button className="btn btn-ghost" onClick={() => (target ? onView(target.id) : onPick())}>
          Ask Claude
        </button>
      </>
    );
  }

  return (
    <div className="hero-body">
      <div className="hero-title">{title}</div>
      <div className="hero-sub">{sub}</div>
      {actions && <div className="hero-actions">{actions}</div>}
    </div>
  );
}

function SessionTile({ s, agents, now, index, onClick }: { s: SessionStatus; agents: Agents; now: number; index: number; onClick: () => void }) {
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
    <Tile
      hum={
        <Hum
          mood={s.running ? (agents.mode === "reckless" ? "fire" : "work") : tone ? toneMood(tone) : "idle"}
          hue={hue(s.project)}
          size={44}
        />
      }
      title={basename(s.project)}
      sub={queued ? `${sub} · ${queued} queued` : sub}
      hint={index < 9 ? `Ctrl+${index + 1}` : undefined}
      tone={s.running ? "busy" : tone}
      style={{ "--h": hue(s.project) } as CSSProperties}
      onClick={onClick}
    />
  );
}

function Tile({
  hum,
  title,
  sub,
  hint,
  tone,
  ghost,
  style,
  onClick,
  onContextMenu,
}: {
  hum: ReactNode;
  title: string;
  sub: string;
  hint?: string;
  tone?: "rec" | "busy" | "ok" | "warn" | "error";
  ghost?: boolean;
  style?: CSSProperties;
  onClick: () => void;
  onContextMenu?: (e: MouseEvent) => void;
}) {
  return (
    <button
      className={`tile ${ghost ? "is-ghost" : ""} ${tone ? `is-${tone}` : ""}`}
      style={style}
      title={sub}
      onClick={onClick}
      onContextMenu={onContextMenu}
    >
      <span className="tile-hum">{hum}</span>
      <span className="tile-title">{title}</span>
      <span className="tile-sub">{sub}</span>
      {hint && <kbd className="tile-hint">{hint}</kbd>}
    </button>
  );
}
