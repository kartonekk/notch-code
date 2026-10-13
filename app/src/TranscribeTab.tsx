import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, AppStatus, AudioSource, Levels, on, Segment } from "./api";
import { CheckIcon, FolderIcon, formatDuration, IconButton, Meter, Orb, PauseIcon, PlayIcon, store } from "./ui";

export const SPEAKER_LABEL = { me: "Me", others: "Others" } as const;

/** Transcription state; lives in App so the collapsed bar can show it too. */
export function useTranscription(status: AppStatus | null, pickerVisible: boolean) {
  const phase = status?.phase ?? "preparing";
  const recording = phase === "recording";
  const paused = recording && !!status?.paused;

  const [sources, setSources] = useState<AudioSource[]>([]);
  const [selectedPid, setSelectedPid] = useState<number | null>(null);
  const [includeMic, setIncludeMicState] = useState(() => store.get("rec.mic") !== "0");
  const [segments, setSegments] = useState<Segment[]>([]);
  const [levels, setLevels] = useState<Levels>({ app: 0, mic: 0 });
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  /** "End" was clicked: asking for the meeting topic (file name) */
  const [ending, setEnding] = useState(false);
  const [subject, setSubject] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const subs = [
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

  // Poll the apps playing audio while the picker is on screen.
  useEffect(() => {
    if (!pickerVisible || phase !== "ready") return;
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
  }, [pickerVisible, phase]);

  const lastLine = useMemo(() => {
    for (let i = segments.length - 1; i >= 0; i--) if (segments[i].text.trim()) return segments[i];
    return null;
  }, [segments]);

  return {
    phase,
    recording,
    paused,
    sources,
    selectedPid,
    setSelectedPid,
    includeMic,
    setIncludeMic: (v: boolean) => {
      setIncludeMicState(v);
      store.set("rec.mic", v ? "1" : "0");
    },
    segments,
    levels,
    error,
    ending,
    setEnding,
    subject,
    setSubject,
    busy,
    lastLine,
    elapsed: status?.started_at ? formatDuration(now - status.started_at) : "00:00",
    start: async () => {
      if (selectedPid === null) return;
      setError(null);
      setSegments([]);
      try {
        await api.start(selectedPid, includeMic);
      } catch (e) {
        setError(String(e));
      }
    },
    finish: async () => {
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
    },
    togglePause: async () => {
      try {
        await api.pause(!paused);
      } catch (e) {
        setError(String(e));
      }
    },
  };
}

export type Transcription = ReturnType<typeof useTranscription>;

/** keys typed into a text field are not shortcuts */
const typing = (e: KeyboardEvent) => e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;

export function TranscribeTab({ t, status }: { t: Transcription; status: AppStatus | null }) {
  if (t.phase === "preparing") return <Preparing status={status} />;
  if (t.phase === "error") {
    return (
      <div className="empty-state">
        <Orb status="error" size={14} />
        <p className="empty-title">The speech model couldn't start</p>
        <p className="error-text">{status?.message}</p>
        <button className="btn primary" onClick={() => api.retrySetup()}>
          Try again
        </button>
      </div>
    );
  }
  return t.recording ? <Live t={t} status={status} /> : <Picker t={t} status={status} />;
}

/**
 * Before recording: the apps playing audio right now (the meeting app is picked
 * already), the microphone switch, and one big Start. Enter starts too.
 */
function Picker({ t, status }: { t: Transcription; status: AppStatus | null }) {
  const selected = t.sources.find((s) => s.pid === t.selectedPid) ?? null;
  const start = useRef(t.start);
  start.current = t.start;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && !typing(e)) {
        e.preventDefault();
        start.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="rec-setup">
      <header className="card-head">
        <span className="card-title">Record audio from</span>
        {status?.backend && <span className="chip">{status.backend}</span>}
        <IconButton title="Transcripts folder" onClick={() => api.openFolder().catch(() => {})}>
          <FolderIcon />
        </IconButton>
      </header>
      <div className="source-list">
        {t.sources.length === 0 && (
          <div className="empty">
            No apps are playing audio.
            <br />
            Join a meeting (Webex, Teams, Zoom…) and it shows up here.
          </div>
        )}
        {t.sources.map((s) => (
          <button
            key={s.pid}
            className={`source ${t.selectedPid === s.pid ? "is-selected" : ""}`}
            title="Click to choose, double-click to start"
            onClick={() => t.setSelectedPid(s.pid)}
            onDoubleClick={() => {
              t.setSelectedPid(s.pid);
              t.start();
            }}
          >
            <span className="source-check">{t.selectedPid === s.pid && <CheckIcon />}</span>
            <span className="source-name">
              {s.name}
              {s.suggested && <span className="chip is-accent">meeting</span>}
            </span>
            <span className="source-exe">{s.exe}</span>
            <Meter level={s.peak} />
          </button>
        ))}
      </div>
      <div className="picker-foot">
        <label className="toggle" title="Your own voice, labelled “Me” in the transcript">
          <input type="checkbox" checked={t.includeMic} onChange={(e) => t.setIncludeMic(e.target.checked)} />
          <span className="switch" />
          Also my microphone
        </label>
        <button className="btn primary lg" disabled={selected === null} onClick={t.start}>
          <span className="rec-dot" />
          <span className="btn-big-text">{selected ? `Start transcribing ${selected.name}` : "Pick an app to record"}</span>
          {selected && <kbd>Enter</kbd>}
        </button>
      </div>
      {t.error && <p className="error-text">{t.error}</p>}
    </div>
  );
}

/**
 * While recording: what's being recorded, how loud, how long, and the three
 * things you do (look at the file, pause, end) on top; the transcript below.
 * Space pauses or resumes.
 */
function Live({ t, status }: { t: Transcription; status: AppStatus | null }) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const [scrolledUp, setScrolledUp] = useState(false);

  useLayoutEffect(() => {
    stick.current = true;
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, []);
  useLayoutEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [t.segments]);

  const pause = useRef(t.togglePause);
  pause.current = t.togglePause;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === " " && !typing(e)) {
        e.preventDefault();
        pause.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const level = t.paused ? 0 : Math.max(t.levels.app, t.levels.mic);
  return (
    <div className="live">
      {t.ending ? (
        <form
          className="live-head live-end"
          onSubmit={(e) => {
            e.preventDefault();
            t.finish();
          }}
        >
          <input
            className="text-input"
            autoFocus
            placeholder="What was the meeting about? (file name, optional)"
            value={t.subject}
            maxLength={80}
            disabled={t.busy}
            onChange={(e) => t.setSubject(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && t.setEnding(false)}
          />
          <button type="button" className="btn secondary" disabled={t.busy} onClick={() => t.setEnding(false)}>
            Keep recording
          </button>
          <button type="submit" className="btn primary" disabled={t.busy}>
            {t.busy ? "Saving…" : "Save"}
          </button>
        </form>
      ) : (
        <div className="live-head">
          <Orb status={t.paused ? "paused" : "rec"} level={level} size={12} />
          <div className="live-meta">
            <span className="live-source" title={status?.source ?? undefined}>
              {t.paused ? "Paused" : "Recording"} {status?.source && <span className="muted">· {status.source}</span>}
            </span>
            <span className="levels">
              <span>App</span>
              <Meter level={t.paused ? 0 : t.levels.app} />
              {t.includeMic && (
                <>
                  <span>Mic</span>
                  <Meter level={t.paused ? 0 : t.levels.mic} />
                </>
              )}
            </span>
          </div>
          <span className={`timer big ${t.paused ? "is-paused" : ""}`}>{t.elapsed}</span>
          <IconButton title="Open the transcript file" onClick={() => api.openTranscript()}>
            <FolderIcon />
          </IconButton>
          <button className="btn secondary" title={`${t.paused ? "Resume" : "Pause"} (Space)`} onClick={t.togglePause}>
            {t.paused ? <PlayIcon /> : <PauseIcon />} {t.paused ? "Resume" : "Pause"}
          </button>
          <button className="btn primary" title="Stop and save the transcript" onClick={() => t.setEnding(true)}>
            <span className="stop-sq" /> End
          </button>
        </div>
      )}
      <div
        className="transcript"
        ref={ref}
        onScroll={(e) => {
          const el = e.currentTarget;
          const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          stick.current = atBottom;
          setScrolledUp(!atBottom);
        }}
      >
        {t.segments.length === 0 && <div className="empty">Waiting for speech…</div>}
        {t.segments.map((s) => (
          <div key={s.id} className={`line speaker-${s.speaker} ${s.final ? "" : "is-partial"}`}>
            <span className="ts">{formatDuration(s.start_ms)}</span>
            <span className="who">{SPEAKER_LABEL[s.speaker]}</span>
            <span className="text">{s.text}</span>
          </div>
        ))}
      </div>
      {scrolledUp && (
        <button
          className="jump-latest"
          onClick={() => {
            const el = ref.current;
            stick.current = true;
            el?.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
          }}
        >
          ↓ Latest
        </button>
      )}
      {t.error && <p className="error-text">{t.error}</p>}
    </div>
  );
}

function Preparing({ status }: { status: AppStatus | null }) {
  const p = status?.progress;
  return (
    <div className="empty-state">
      <Orb status="setup" size={14} />
      <p className="empty-title">{status?.message ?? "Preparing…"}</p>
      {p != null && (
        <div className="progress">
          <div className="progress-fill" style={{ width: `${Math.round(p * 100)}%` }} />
        </div>
      )}
      <p className="muted small">The first launch downloads the speech recognition model (one time only).</p>
    </div>
  );
}
