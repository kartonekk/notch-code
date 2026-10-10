import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, AppStatus, AudioSource, Levels, on, Segment } from "./api";
import "./notch.css";

// Window sizes in logical pixels. The window is resized to match the notch,
// because transparent parts of a WebView window still swallow mouse clicks.
const COLLAPSED = { w: 240, h: 36 };
const COLLAPSED_LIVE = { w: 460, h: 36 };
const EXPANDED = { w: 580, h: 420 };
const ANIM_MS = 260;

const SPEAKER_LABEL = { me: "Ja", others: "Rozmówcy" } as const;

export default function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [sources, setSources] = useState<AudioSource[]>([]);
  const [selectedPid, setSelectedPid] = useState<number | null>(null);
  const [includeMic, setIncludeMic] = useState(true);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [levels, setLevels] = useState<Levels>({ app: 0, mic: 0 });
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  /** "Zakończ" was clicked: asking for the meeting subject (file name) */
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

  // `shell` keeps the expanded panel mounted (and the window large) until the
  // collapse animation has finished, then the window shrinks to the bar.
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
        onMouseEnter={onEnter}
        onMouseLeave={onLeave}
        onClick={() => !expanded && expand()}
      >
        <span className="ear ear-l" />
        <span className="ear ear-r" />
        <div className="notch-clip">
        {/* ---------- collapsed bar ---------- */}
        <div className="bar">
          <StatusDot phase={paused ? "paused" : phase} />
          {recording ? (
            <>
              <span className={`timer ${paused ? "is-paused" : ""}`}>{elapsed}</span>
              <span className="ticker">
                {paused ? (
                  <span className="muted">Wstrzymano — najedź, aby wznowić</span>
                ) : lastLine ? (
                  <>
                    <b>{SPEAKER_LABEL[lastLine.speaker]}:</b> {lastLine.text}
                  </>
                ) : (
                  <span className="muted">Słucham…</span>
                )}
              </span>
              <Bars level={paused ? 0 : Math.max(levels.app, levels.mic)} />
            </>
          ) : (
            <span className="bar-label">
              {phase === "preparing"
                ? status?.progress != null
                  ? `Pobieranie modelu ${Math.round(status.progress * 100)}%`
                  : "Przygotowanie…"
                : phase === "error"
                  ? "Błąd — najedź, aby zobaczyć"
                  : "Transkrypt"}
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
                    {paused ? "Wstrzymano" : "Nagrywanie"} <span className="muted">· {status?.source}</span>
                  </>
                ) : (
                  "Transkrypt"
                )}
              </div>
              <div className="head-actions">
                {status?.backend && <span className="chip">{status.backend}</span>}
                <IconButton title={pinned ? "Odepnij" : "Przypnij otwarty"} active={pinned} onClick={() => setPinned(!pinned)}>
                  <PinIcon />
                </IconButton>
                <IconButton title="Folder z transkryptami" onClick={() => api.openFolder()}>
                  <FolderIcon />
                </IconButton>
                <IconButton title="Zamknij aplikację" onClick={() => api.quit()}>
                  <PowerIcon />
                </IconButton>
              </div>
            </header>

            {phase === "preparing" && <Preparing status={status} />}

            {phase === "error" && (
              <div className="center-box">
                <p className="error-text">{status?.message}</p>
                <button className="btn" onClick={() => api.retrySetup()}>
                  Spróbuj ponownie
                </button>
              </div>
            )}

            {phase === "ready" && (
              <div className="picker">
                <div className="section-label">Z której aplikacji nagrywać dźwięk?</div>
                <div className="source-list">
                  {sources.length === 0 && (
                    <div className="empty">
                      Brak aplikacji odtwarzających dźwięk.
                      <br />
                      Dołącz do spotkania Webex — pojawi się tutaj.
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
                        {s.suggested && <span className="chip chip-accent">spotkanie</span>}
                      </span>
                      <span className="source-exe">{s.exe}</span>
                      <Meter level={s.peak} />
                    </button>
                  ))}
                </div>
                <label className="toggle">
                  <input type="checkbox" checked={includeMic} onChange={(e) => setIncludeMic(e.target.checked)} />
                  <span className="switch" />
                  Nagrywaj też mój mikrofon (podpis „Ja”)
                </label>
                {error && <p className="error-text">{error}</p>}
                <button className="btn btn-record" disabled={selectedPid === null} onClick={start}>
                  <span className="rec-dot" /> Rozpocznij transkrypcję
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
                  {segments.length === 0 && <div className="empty">Czekam na mowę…</div>}
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
                    ↓ Najnowsze
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
                      placeholder="Temat spotkania (nazwa pliku) — opcjonalnie"
                      value={subject}
                      maxLength={80}
                      disabled={busy}
                      onChange={(e) => setSubject(e.target.value)}
                      onKeyDown={(e) => e.key === "Escape" && setEnding(false)}
                    />
                    <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setEnding(false)}>
                      Anuluj
                    </button>
                    <button type="submit" className="btn btn-stop" disabled={busy}>
                      {busy ? "Zapisuję…" : "Zakończ i zapisz"}
                    </button>
                  </form>
                ) : (
                  <footer className="live-foot">
                    <div className="levels">
                      <span>Aplikacja</span>
                      <Meter level={paused ? 0 : levels.app} />
                      {includeMic && (
                        <>
                          <span>Mikrofon</span>
                          <Meter level={paused ? 0 : levels.mic} />
                        </>
                      )}
                    </div>
                    <span className={`timer big ${paused ? "is-paused" : ""}`}>{paused ? "Pauza" : elapsed}</span>
                    <button className="btn btn-ghost" title="Otwórz plik transkryptu" onClick={() => api.openTranscript()}>
                      Plik
                    </button>
                    <button className="btn" onClick={togglePause}>
                      {paused ? <PlayIcon /> : <PauseIcon />} {paused ? "Wznów" : "Pauza"}
                    </button>
                    <button className="btn btn-stop" onClick={() => setEnding(true)}>
                      <span className="stop-sq" /> Zakończ
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
      <p>{status?.message ?? "Przygotowanie…"}</p>
      {p != null && (
        <div className="progress">
          <div className="progress-fill" style={{ width: `${Math.round(p * 100)}%` }} />
        </div>
      )}
      <p className="muted small">Pierwsze uruchomienie pobiera model rozpoznawania mowy (jednorazowo).</p>
    </div>
  );
}

function StatusDot({ phase }: { phase: string }) {
  return <span className={`dot dot-${phase}`} />;
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
