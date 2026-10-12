import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, AppStatus, AudioSource, Levels, on, Segment } from "./api";
import { formatDuration, Meter, PauseIcon, PlayIcon, store } from "./ui";

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

export function TranscribeTab({ t, status }: { t: Transcription; status: AppStatus | null }) {
  if (t.phase === "preparing") return <Preparing status={status} />;
  if (t.phase === "error") {
    return (
      <div className="empty-state">
        <p className="error-text">{status?.message}</p>
        <button className="btn" onClick={() => api.retrySetup()}>
          Try again
        </button>
      </div>
    );
  }
  return t.recording ? <Live t={t} /> : <Picker t={t} />;
}

function Picker({ t }: { t: Transcription }) {
  return (
    <div className="picker">
      <div className="section-label">Record audio from</div>
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
            onClick={() => t.setSelectedPid(s.pid)}
            onDoubleClick={() => {
              t.setSelectedPid(s.pid);
              t.start();
            }}
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
      <div className="picker-foot">
        <label className="toggle">
          <input type="checkbox" checked={t.includeMic} onChange={(e) => t.setIncludeMic(e.target.checked)} />
          <span className="switch" />
          Also my microphone
        </label>
        <button className="btn btn-record" disabled={t.selectedPid === null} onClick={t.start}>
          <span className="rec-dot" /> Start transcribing
        </button>
      </div>
      {t.error && <p className="error-text">{t.error}</p>}
    </div>
  );
}

function Live({ t }: { t: Transcription }) {
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

  return (
    <div className="live">
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
      {scrolledUp && !t.ending && (
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
      {t.ending ? (
        <form
          className="live-foot"
          onSubmit={(e) => {
            e.preventDefault();
            t.finish();
          }}
        >
          <input
            className="text-input"
            autoFocus
            placeholder="Meeting topic (file name) — optional"
            value={t.subject}
            maxLength={80}
            disabled={t.busy}
            onChange={(e) => t.setSubject(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && t.setEnding(false)}
          />
          <button type="button" className="btn btn-ghost" disabled={t.busy} onClick={() => t.setEnding(false)}>
            Cancel
          </button>
          <button type="submit" className="btn btn-stop" disabled={t.busy}>
            {t.busy ? "Saving…" : "Save"}
          </button>
        </form>
      ) : (
        <footer className="live-foot">
          <div className="levels">
            <span>App</span>
            <Meter level={t.paused ? 0 : t.levels.app} />
            {t.includeMic && (
              <>
                <span>Mic</span>
                <Meter level={t.paused ? 0 : t.levels.mic} />
              </>
            )}
          </div>
          <span className={`timer big ${t.paused ? "is-paused" : ""}`}>{t.paused ? "Paused" : t.elapsed}</span>
          <button className="btn btn-ghost" title="Open the transcript file" onClick={() => api.openTranscript()}>
            Open file
          </button>
          <button className="btn" onClick={t.togglePause}>
            {t.paused ? <PlayIcon /> : <PauseIcon />} {t.paused ? "Resume" : "Pause"}
          </button>
          <button className="btn btn-stop" onClick={() => t.setEnding(true)}>
            <span className="stop-sq" /> End
          </button>
        </footer>
      )}
      {t.error && <p className="error-text">{t.error}</p>}
    </div>
  );
}

function Preparing({ status }: { status: AppStatus | null }) {
  const p = status?.progress;
  return (
    <div className="empty-state">
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
