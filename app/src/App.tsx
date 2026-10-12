import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, AppStatus, AudioSource, Levels, NetStatus, on, Segment } from "./api";
import "./notch.css";

// Notch sizes in logical pixels. The window itself always has the EXPANDED size
// (see WINDOW_W in lib.rs); the backend makes everything outside the current
// notch click-through and reports hover via the `notch-hover` event.
const COLLAPSED = { w: 270, h: 36 };
const COLLAPSED_LIVE = { w: 460, h: 36 };
const EXPANDED = { w: 580, h: 420 };
const ANIM_MS = 260;

const SPEAKER_LABEL = { me: "Me", others: "Others" } as const;

export default function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [sources, setSources] = useState<AudioSource[]>([]);
  const [selectedPid, setSelectedPid] = useState<number | null>(null);
  const [includeMic, setIncludeMic] = useState(true);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [levels, setLevels] = useState<Levels>({ app: 0, mic: 0 });
  const [net, setNet] = useState<NetStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  /** "End" was clicked: asking for the meeting subject (file name) */
  const [ending, setEnding] = useState(false);
  const [subject, setSubject] = useState("");
  const [busy, setBusy] = useState(false);
  const [scrolledUp, setScrolledUp] = useState(false);

  const collapseTimer = useRef<number | null>(null);
  const hoverTimer = useRef<number | null>(null);
  const transcriptRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

  const phase = status?.phase ?? "preparing";
  const recording = phase === "recording";
  const paused = recording && !!status?.paused;

  // ---- backend wiring -------------------------------------------------------
  useEffect(() => {
    api.status().then(setStatus).catch(() => {});
    const subs = [
      on<AppStatus>("status", setStatus),
      on<Levels>("levels", setLevels),
      on<NetStatus>("net", setNet),
      on<Segment>("segment", (seg) =>
        setSegments((prev) => {
          const i = prev.findIndex((s) => s.id === seg.id);
          if (i === -1) return [...prev, seg].slice(-3000);
          const next = prev.slice();
          next[i] = seg;
          return next;
        }),
      ),
    ];
    return () => subs.forEach((p) => p.then((un) => un()));
  }, []);

  useEffect(() => {
    if (!recording) return;
    const t = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(t);
  }, [recording]);

  // Poll the list of apps playing audio while the picker is visible.
  useEffect(() => {
    if (!expanded || phase !== "ready") return;
    let alive = true;
    const load = () =>
      api
        .sources()
        .then((list) => {
          if (!alive) return;
          setSources(list);
          setSelectedPid((cur) => {
            if (cur !== null && list.some((s) => s.pid === cur)) return cur;
            return (list.find((s) => s.suggested) ?? list.find((s) => s.active) ?? list[0])?.pid ?? null;
          });
        })
        .catch((e) => setError(String(e)));
    load();
    const t = window.setInterval(load, 1200);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [expanded, phase]);

  // ---- notch geometry -------------------------------------------------------
  const collapsedSize = recording ? COLLAPSED_LIVE : COLLAPSED;

  const clearTimers = () => {
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    collapseTimer.current = hoverTimer.current = null;
  };

  const expand = useCallback(async () => {
    clearTimers();
    await api.setNotchSize(EXPANDED.w, EXPANDED.h);
    setExpanded(true);
  }, []);

  const collapse = useCallback(() => {
    clearTimers();
    setExpanded(false);
  }, []);

  const onEnter = () => {
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    collapseTimer.current = null;
    if (!expanded && !hoverTimer.current) hoverTimer.current = window.setTimeout(expand, 140);
  };
  const onLeave = () => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    hoverTimer.current = null;
    // stay open while pinned or while the subject is being typed
    if (expanded && !pinned && !ending) collapseTimer.current = window.setTimeout(collapse, 600);
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
      // small buffer so the CSS transition has fully settled before the window shrinks
      const t = window.setTimeout(() => setShell(false), ANIM_MS + 40);
      return () => window.clearTimeout(t);
    }
  }, [expanded]);
  useEffect(() => {
    if (!shell && !expanded) api.setNotchSize(collapsedSize.w, collapsedSize.h);
  }, [shell, expanded, collapsedSize.w, collapsedSize.h]);

  // ---- transcript scroll ----------------------------------------------------
  // Every time the panel opens, start at the newest line; scrolling up goes
  // back in time. While at the bottom, new lines keep it pinned there.
  useLayoutEffect(() => {
    if (!shell || !recording) return;
    stickToBottom.current = true;
    setScrolledUp(false);
    const el = transcriptRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [shell, recording]);
  useLayoutEffect(() => {
    const el = transcriptRef.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
  }, [segments]);
  const scrollToBottom = () => {
    const el = transcriptRef.current;
    if (!el) return;
    stickToBottom.current = true;
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  };

  // ---- actions --------------------------------------------------------------
  const start = async () => {
    if (selectedPid === null) return;
    setError(null);
    setSegments([]);
    try {
      await api.start(selectedPid, includeMic);
    } catch (e) {
      setError(String(e));
    }
  };
  const finish = async () => {
    setBusy(true);
    try {
      await api.stop(subject.trim() || null);
      setEnding(false);
      setSubject("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const togglePause = async () => {
    try {
      await api.pause(!paused);
    } catch (e) {
      setError(String(e));
    }
  };

  // ---- derived --------------------------------------------------------------
  const elapsed = status?.started_at ? formatDuration(now - status.started_at) : "00:00";
  const lastLine = useMemo(() => {
    for (let i = segments.length - 1; i >= 0; i--) if (segments[i].text.trim()) return segments[i];
    return null;
  }, [segments]);

  const size = expanded ? EXPANDED : collapsedSize;

  return (
    <div className="stage">
      <div
        className={`notch ${expanded ? "is-expanded" : ""} phase-${phase}`}
        style={{ width: size.w, height: size.h }}
        onClick={() => !expanded && expand()}
      >
        <span className="ear ear-l" />
        <span className="ear ear-r" />
        <div className="notch-clip">
        {/* ---------- collapsed bar ---------- */}
        <div className="bar">
          <span className="bar-lead">
            <NetIndicator net={net} />
            <StatusDot phase={paused ? "paused" : phase} />
          </span>
          {recording ? (
            <>
              <span className={`timer ${paused ? "is-paused" : ""}`}>{elapsed}</span>
              <span className="ticker">
                {paused ? (
                  <span className="muted">Paused — hover to resume</span>
                ) : lastLine ? (
                  <>
                    <b>{SPEAKER_LABEL[lastLine.speaker]}:</b> {lastLine.text}
                  </>
                ) : (
                  <span className="muted">Listening…</span>
                )}
              </span>
              <Bars level={paused ? 0 : Math.max(levels.app, levels.mic)} />
            </>
          ) : (
            <span className="bar-label">
              {phase === "preparing"
                ? status?.progress != null
                  ? `Downloading model ${Math.round(status.progress * 100)}%`
                  : "Preparing…"
                : phase === "error"
                  ? "Error — hover to see"
                  : net && !net.online
                    ? "No internet"
                    : "Notch Code"}
            </span>
          )}
        </div>

        {/* ---------- expanded panel ---------- */}
        {shell && (
          <div className="panel" onClick={(e) => e.stopPropagation()}>
            <header className="panel-head">
              <div className="title">
                {recording ? (
                  <>
                    {paused ? "Paused" : "Recording"} <span className="muted">· {status?.source}</span>
                  </>
                ) : (
                  "Notch Code"
                )}
              </div>
              <div className="head-actions">
                <NetIndicator net={net} showLatency />
                {status?.backend && <span className="chip">{status.backend}</span>}
                <IconButton title={pinned ? "Unpin" : "Keep open"} active={pinned} onClick={() => setPinned(!pinned)}>
                  <PinIcon />
                </IconButton>
                <IconButton title="Transcripts folder" onClick={() => api.openFolder()}>
                  <FolderIcon />
                </IconButton>
                <IconButton title="Quit" onClick={() => api.quit()}>
                  <PowerIcon />
                </IconButton>
              </div>
            </header>

            {phase === "preparing" && <Preparing status={status} />}

            {phase === "error" && (
              <div className="center-box">
                <p className="error-text">{status?.message}</p>
                <button className="btn" onClick={() => api.retrySetup()}>
                  Try again
                </button>
              </div>
            )}

            {phase === "ready" && (
              <div className="picker">
                <div className="section-label">Which app should audio be recorded from?</div>
                <div className="source-list">
                  {sources.length === 0 && (
                    <div className="empty">
                      No apps are playing audio.
                      <br />
                      Join a Webex meeting — it will show up here.
                    </div>
                  )}
                  {sources.map((s) => (
                    <button
                      key={s.pid}
                      className={`source ${selectedPid === s.pid ? "is-selected" : ""}`}
                      onClick={() => setSelectedPid(s.pid)}
                    >
                      <span className="source-name">
                        {s.name}
                        {s.suggested && <span className="chip chip-accent">meeting</span>}
                      </span>
                      <span className="source-exe">{s.exe}</span>
                      <Meter level={s.peak} />
                    </button>
                  ))}
                </div>
                <label className="toggle">
                  <input type="checkbox" checked={includeMic} onChange={(e) => setIncludeMic(e.target.checked)} />
                  <span className="switch" />
                  Also record my microphone (labelled “Me”)
                </label>
                {error && <p className="error-text">{error}</p>}
                <button className="btn btn-record" disabled={selectedPid === null} onClick={start}>
                  <span className="rec-dot" /> Start transcribing
                </button>
              </div>
            )}

            {recording && (
              <div className="live">
                <div
                  className="transcript"
                  ref={transcriptRef}
                  onScroll={(e) => {
                    const el = e.currentTarget;
                    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
                    stickToBottom.current = atBottom;
                    setScrolledUp(!atBottom);
                  }}
                >
                  {segments.length === 0 && <div className="empty">Waiting for speech…</div>}
                  {segments.map((s) => (
                    <div key={s.id} className={`line speaker-${s.speaker} ${s.final ? "" : "is-partial"}`}>
                      <span className="ts">{formatDuration(s.start_ms)}</span>
                      <span className="who">{SPEAKER_LABEL[s.speaker]}</span>
                      <span className="text">{s.text}</span>
                    </div>
                  ))}
                </div>
                {scrolledUp && !ending && (
                  <button className="jump-latest" onClick={scrollToBottom}>
                    ↓ Latest
                  </button>
                )}
                {ending ? (
                  <form
                    className="live-foot end-form"
                    onSubmit={(e) => {
                      e.preventDefault();
                      finish();
                    }}
                  >
                    <input
                      className="subject-input"
                      autoFocus
                      placeholder="Meeting topic (file name) — optional"
                      value={subject}
                      maxLength={80}
                      disabled={busy}
                      onChange={(e) => setSubject(e.target.value)}
                      onKeyDown={(e) => e.key === "Escape" && setEnding(false)}
                    />
                    <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setEnding(false)}>
                      Cancel
                    </button>
                    <button type="submit" className="btn btn-stop" disabled={busy}>
                      {busy ? "Saving…" : "End and save"}
                    </button>
                  </form>
                ) : (
                  <footer className="live-foot">
                    <div className="levels">
                      <span>App</span>
                      <Meter level={paused ? 0 : levels.app} />
                      {includeMic && (
                        <>
                          <span>Microphone</span>
                          <Meter level={paused ? 0 : levels.mic} />
                        </>
                      )}
                    </div>
                    <span className={`timer big ${paused ? "is-paused" : ""}`}>{paused ? "Paused" : elapsed}</span>
                    <button className="btn btn-ghost" title="Open transcript file" onClick={() => api.openTranscript()}>
                      File
                    </button>
                    <button className="btn" onClick={togglePause}>
                      {paused ? <PlayIcon /> : <PauseIcon />} {paused ? "Resume" : "Pause"}
                    </button>
                    <button className="btn btn-stop" onClick={() => setEnding(true)}>
                      <span className="stop-sq" /> End
                    </button>
                  </footer>
                )}
                {error && <p className="error-text">{error}</p>}
              </div>
            )}
          </div>
        )}
        </div>
      </div>
    </div>
  );
}

// ---- small components -------------------------------------------------------

function Preparing({ status }: { status: AppStatus | null }) {
  const p = status?.progress;
  return (
    <div className="center-box">
      <div className="spinner" />
      <p>{status?.message ?? "Preparing…"}</p>
      {p != null && (
        <div className="progress">
          <div className="progress-fill" style={{ width: `${Math.round(p * 100)}%` }} />
        </div>
      )}
      <p className="muted small">The first launch downloads the speech recognition model (one time only).</p>
    </div>
  );
}

function StatusDot({ phase }: { phase: string }) {
  return <span className={`dot dot-${phase}`} />;
}

function NetIndicator({ net, showLatency }: { net: NetStatus | null; showLatency?: boolean }) {
  const state = !net ? "unknown" : !net.online ? "offline" : (net.latency_ms ?? 0) > 300 ? "slow" : "online";
  const title = !net
    ? "Checking connection…"
    : net.online
      ? `Internet: ${net.latency_ms} ms`
      : "No internet connection";
  return (
    <span className={`net net-${state}`} title={title}>
      {state === "offline" ? <WifiOffIcon /> : <WifiIcon />}
      {showLatency && <span className="net-text">{net?.online ? `${net.latency_ms} ms` : state === "offline" ? "offline" : "…"}</span>}
    </span>
  );
}

function Bars({ level }: { level: number }) {
  const l = Math.min(1, Math.sqrt(level) * 1.6);
  const weights = [0.55, 0.85, 1, 0.75, 0.5];
  return (
    <span className="bars">
      {weights.map((w, i) => (
        <i key={i} style={{ transform: `scaleY(${0.18 + l * w * 0.82})` }} />
      ))}
    </span>
  );
}

function Meter({ level }: { level: number }) {
  const l = Math.min(1, Math.sqrt(level) * 1.4);
  return (
    <span className="meter">
      <i style={{ width: `${l * 100}%` }} />
    </span>
  );
}

function IconButton(props: { title: string; onClick: () => void; active?: boolean; children: React.ReactNode }) {
  return (
    <button className={`icon-btn ${props.active ? "is-active" : ""}`} title={props.title} onClick={props.onClick}>
      {props.children}
    </button>
  );
}

const svg = { width: 15, height: 15, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round" } as const;
const PinIcon = () => (
  <svg {...svg}>
    <path d="M12 17v5M9 3h6l-1 7 4 3v2H6v-2l4-3-1-7z" />
  </svg>
);
const FolderIcon = () => (
  <svg {...svg}>
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
  </svg>
);
const PauseIcon = () => (
  <svg {...svg} width={13} height={13}>
    <path d="M8 5v14M16 5v14" />
  </svg>
);
const PlayIcon = () => (
  <svg {...svg} width={13} height={13}>
    <path d="M7 5l12 7-12 7z" />
  </svg>
);
const WifiIcon = () => (
  <svg {...svg} width={14} height={14}>
    <path d="M2 8.8a15 15 0 0 1 20 0M5 12.6a10 10 0 0 1 14 0M8.5 16.3a5 5 0 0 1 7 0" />
    <circle cx="12" cy="20" r="0.6" fill="currentColor" />
  </svg>
);
const WifiOffIcon = () => (
  <svg {...svg} width={14} height={14}>
    <path d="M2 2l20 20M8.5 16.3a5 5 0 0 1 7 0M5 12.6a10 10 0 0 1 5.2-2.7M16.7 10.4A10 10 0 0 1 19 12.6M2 8.8a15 15 0 0 1 4.6-2.9M10.7 5a15 15 0 0 1 11.3 3.8" />
    <circle cx="12" cy="20" r="0.6" fill="currentColor" />
  </svg>
);
const PowerIcon = () => (
  <svg {...svg}>
    <path d="M12 3v9M6.3 6.3a8 8 0 1 0 11.4 0" />
  </svg>
);

function formatDuration(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(sec).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}
