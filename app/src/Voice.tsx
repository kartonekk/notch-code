import { useEffect, useState } from "react";
import { api, on, VoiceStatus } from "./api";
import { CrossIcon, MicIcon } from "./ui";

/** Spoken prompts: the wake word "Claude" and the mic button. */
export function useVoice() {
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.voiceStatus().then(setStatus).catch(() => {});
    const sub = on<VoiceStatus>("voice", setStatus);
    return () => {
      sub.then((un) => un());
    };
  }, []);

  const call = (p: Promise<unknown>) => {
    setError(null);
    p.catch((e) => setError(String(e)));
  };
  const phase = status?.phase ?? "off";
  return {
    phase,
    wake: !!status?.wake,
    /** what was heard of the prompt so far */
    text: status?.text ?? null,
    error: error ?? status?.error ?? null,
    /** waiting for or taking a prompt */
    active: phase === "armed" || phase === "hearing",
    /** the mic button: talk now (no wake word), again to cancel */
    talk: () => call(api.voiceTalk()),
    setWake: (on: boolean) => call(api.voiceSetWake(on)),
  };
}

export type Voice = ReturnType<typeof useVoice>;

/** "Listening…" line while a prompt is being taken. */
export function voiceLine(v: Voice) {
  if (v.phase === "hearing" && v.text) return v.text;
  return v.phase === "armed" ? "Listening — say what Claude should do" : "Listening…";
}

/** The mic button next to send (only while voice is on). */
export function MicButton({ voice }: { voice: Voice }) {
  if (!voice.wake && !voice.active) return null;
  return (
    <button
      type="button"
      className={`mic-btn ${voice.active ? "is-live" : ""}`}
      title={voice.active ? "Cancel the spoken prompt" : "Say the prompt — sent when you stop talking"}
      aria-label={voice.active ? "Cancel the spoken prompt" : "Speak a prompt"}
      onClick={voice.talk}
    >
      <MicIcon />
    </button>
  );
}

/** Above the composer while a spoken prompt is being taken. */
export function VoiceStrip({ voice }: { voice: Voice }) {
  if (!voice.active && !voice.error) return null;
  if (!voice.active) return <p className="error-text">{voice.error}</p>;
  return (
    <div className={`voice-strip is-${voice.phase}`}>
      <span className="voice-wave" aria-hidden>
        <i />
        <i />
        <i />
      </span>
      <span className={`voice-text ${voice.phase === "hearing" && voice.text ? "" : "muted"}`}>{voiceLine(voice)}</span>
      <span className="voice-note">sent when you stop talking</span>
      <button className="queued-x" title="Cancel" onClick={voice.talk}>
        <CrossIcon />
      </button>
    </div>
  );
}
