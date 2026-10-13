import { useEffect, useState } from "react";
import { api, Media, on } from "./api";
import { formatDuration, MusicIcon, NextIcon, PauseIcon, PlayIcon, PrevIcon } from "./ui";

/** What's playing on this computer, as Windows' media controls report it; null when nothing is. */
export function useMedia() {
  const [media, setMedia] = useState<Media | null>(null);
  useEffect(() => {
    api.mediaStatus().then(setMedia).catch(() => {});
    const sub = on<Media | null>("media", setMedia);
    return () => {
      sub.then((un) => un());
    };
  }, []);
  return media;
}

/** where the track is now: the last reported spot, moved on while it plays */
function position(m: Media, now: number) {
  if (m.position_ms == null) return null;
  const moved = m.playing && m.updated_at ? Math.max(0, now - m.updated_at) : 0;
  return Math.min(m.duration_ms ?? Infinity, m.position_ms + moved);
}

/** Home's "now playing" card: art, title, where it is, and play / pause / skip. */
export function NowPlaying({ media }: { media: Media }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!media.playing) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [media.playing]);
  // the button flips right away; the next report from Windows settles it
  const [wanted, setWanted] = useState<boolean | null>(null);
  useEffect(() => setWanted(null), [media.playing, media.title]);
  const playing = wanted ?? media.playing;

  const pos = position(media, now);
  const pct = pos != null && media.duration_ms ? Math.min(100, (pos / media.duration_ms) * 100) : null;
  const control = (action: "toggle" | "next" | "prev") => api.mediaControl(action).catch(() => {});

  return (
    <section className={`card now-playing ${playing ? "is-playing" : ""}`} title={`${media.title}${media.artist ? ` — ${media.artist}` : ""} · ${media.app}`}>
      {media.thumb ? (
        <img className="np-art" src={media.thumb} alt="" draggable={false} />
      ) : (
        <span className="np-art is-empty">
          <MusicIcon />
        </span>
      )}
      <div className="np-body">
        <div className="np-title">{media.title}</div>
        <div className="np-sub">
          {media.artist && <span className="np-artist">{media.artist}</span>}
          <span className="np-app">{media.app}</span>
        </div>
        {pct != null && (
          <div className="np-time">
            <span className="np-bar">
              <i style={{ width: `${pct}%` }} />
            </span>
            <span className="np-clock">
              {formatDuration(pos!)} / {formatDuration(media.duration_ms!)}
            </span>
          </div>
        )}
      </div>
      <div className="np-controls">
        <button className="np-btn" title="Previous" aria-label="Previous" disabled={!media.can_prev} onClick={() => control("prev")}>
          <PrevIcon />
        </button>
        <button
          className="np-btn np-play"
          title={playing ? "Pause" : "Play"}
          aria-label={playing ? "Pause" : "Play"}
          onClick={() => {
            setWanted(!playing);
            control("toggle");
          }}
        >
          {playing ? <PauseIcon /> : <PlayIcon />}
        </button>
        <button className="np-btn" title="Next" aria-label="Next" disabled={!media.can_next} onClick={() => control("next")}>
          <NextIcon />
        </button>
      </div>
    </section>
  );
}
