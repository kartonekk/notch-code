import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, isAudioPath, on, RecordingItem, RecordingsStatus } from "./api";
import { CrossIcon, formatDuration } from "./ui";

/**
 * Recorded files to transcribe (a phone recording of a lab, an .mp3 from someone).
 * Dropping audio anywhere on the open island queues it, whatever tab is shown;
 * `onDropped` then brings up the Transcribe tab. Images still go to Claude.
 */
export function useRecordings(onDropped: () => void) {
  const [status, setStatus] = useState<RecordingsStatus>({ items: [], note: null });
  const [error, setError] = useState<string | null>(null);
  /** audio files are being dragged over the island */
  const [dragging, setDragging] = useState(false);

  const add = (paths: string[]) => {
    setError(null);
    api.recordingsAdd(paths).catch((e) => setError(String(e)));
  };

  useEffect(() => {
    api.recordingsStatus().then(setStatus).catch(() => {});
    const sub = on<RecordingsStatus>("recordings", setStatus);
    return () => {
      sub.then((un) => un());
    };
  }, []);

  useEffect(() => {
    const sub = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter") setDragging(p.paths.some(isAudioPath));
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        const audio = p.paths.filter(isAudioPath);
        if (audio.length) {
          add(audio);
          onDropped();
        }
      }
    });
    return () => {
      sub.then((un) => un());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const items = status.items;
  return {
    items,
    note: status.note,
    error,
    dragging,
    /** a file is waiting or being transcribed */
    busy: items.some((i) => i.state === "waiting" || i.state === "working"),
    working: items.find((i) => i.state === "working") ?? null,
    pick: () => {
      setError(null);
      api.recordingsPick().catch((e) => setError(String(e)));
    },
    remove: (id: number) => api.recordingsRemove(id).catch(() => {}),
    clear: () => api.recordingsClear().catch(() => {}),
    open: (id: number) => api.recordingsOpen(id).catch((e) => setError(String(e))),
  };
}

export type Recordings = ReturnType<typeof useRecordings>;

/** Height the list needs in the Transcribe tab (App sizes the island with it). */
export function recordingsHeight(r: Recordings) {
  return 58 + Math.min(r.items.length, 3) * 48 + (r.items.length > 1 ? 22 : 0) + (r.note || r.error ? 20 : 0);
}

/**
 * Under the meeting apps: the files in the queue, then a dashed drop target
 * that also opens the file dialog when clicked.
 */
export function RecordingsPanel({ r }: { r: Recordings }) {
  const finished = r.items.filter((i) => i.state !== "waiting" && i.state !== "working").length;
  return (
    <div className="recordings">
      {r.items.length > 0 && (
        <div className="rec-files">
          {r.items.map((item) => (
            <FileRow key={item.id} item={item} r={r} />
          ))}
        </div>
      )}
      {r.items.length > 1 && finished > 0 && (
        <button className="rec-clear" onClick={r.clear}>
          Clear finished
        </button>
      )}
      <button className={`drop-zone ${r.dragging ? "is-over" : ""}`} onClick={r.pick} title="MP3, M4A, WAV, FLAC or OGG">
        <WaveIcon />
        <span className="drop-text">
          <b>{r.dragging ? "Drop to transcribe" : "Transcribe a recording"}</b>
          <span className="muted">Drop an audio file here, or click to browse · MP3, M4A, WAV, FLAC, OGG</span>
        </span>
      </button>
      {r.note && <p className="muted small rec-note">{r.note}</p>}
      {r.error && <p className="error-text">{r.error}</p>}
    </div>
  );
}

function FileRow({ item, r }: { item: RecordingItem; r: Recordings }) {
  const length = item.duration_ms != null ? formatDuration(item.duration_ms) : null;
  const pct = item.progress != null ? Math.round(item.progress * 100) : null;
  const line =
    item.state === "waiting"
      ? ["Waiting", length].filter(Boolean).join(" · ")
      : item.state === "working"
        ? ["Transcribing", pct != null ? `${pct}%` : null, length].filter(Boolean).join(" · ")
        : item.state === "done"
          ? ["Done", length].filter(Boolean).join(" · ")
          : item.state === "cancelled"
            ? "Stopped · the part so far is saved"
            : (item.error ?? "Failed");
  const canOpen = item.transcript != null && item.state !== "failed";
  return (
    <div className={`rec-file is-${item.state}`} title={item.path}>
      <span className="rec-file-icon">
        {item.state === "working" ? <span className="mini-spinner" /> : item.state === "done" ? <span className="state-dot ok" /> : <WaveIcon small />}
      </span>
      <span className="rec-file-text">
        <span className="rec-file-name">{item.name}</span>
        <span className={`rec-file-line ${item.state === "failed" ? "error-text" : "muted"}`}>{line}</span>
      </span>
      {canOpen && (
        <button className="btn secondary sm" onClick={() => r.open(item.id)} title="Open the transcript">
          Open
        </button>
      )}
      <button
        className="icon-btn"
        title={item.state === "working" ? "Stop (keeps what's transcribed so far)" : item.state === "waiting" ? "Don't transcribe" : "Remove from the list"}
        aria-label="Remove"
        onClick={() => r.remove(item.id)}
      >
        <CrossIcon />
      </button>
      {item.state === "working" && (
        <span className="rec-file-progress">
          <span className={`rec-file-fill ${pct == null ? "is-unknown" : ""}`} style={{ width: pct != null ? `${pct}%` : undefined }} />
        </span>
      )}
    </div>
  );
}

/** Covers the open island while audio is dragged over it, whatever tab is shown. */
export function DropOverlay({ r }: { r: Recordings }) {
  if (!r.dragging) return null;
  return (
    <div className="drop-overlay">
      <WaveIcon />
      <b>Drop to transcribe</b>
      <span className="muted">Saved as Markdown in Documents\Notch Code</span>
    </div>
  );
}

const WaveIcon = ({ small }: { small?: boolean }) => (
  <svg width={small ? 13 : 18} height={small ? 13 : 18} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round">
    <path d="M3 10v4M7 6v12M11 3v18M15 8v8M19 5v14" />
  </svg>
);
