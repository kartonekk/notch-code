import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { Status, store } from "./ui";

/**
 * The mascot skins (concepts in design/mascots.html). Each one is a small SVG
 * scene (240 × 220, its top edge is the top of the screen) whose glowing parts
 * take the status colour: `.acc` fills and `.acc-s` strokes use --accent, which
 * follows the `is-<status>` class on the mascot.
 */
export type MascotId = "none" | "notchy" | "bitwing" | "hoot" | "fen" | "jelly" | "purr" | "prompt" | "whisper" | "slo" | "echo";

const KEY = "mascot";
const DEFAULT: MascotId = "notchy";

/** everyone showing the mascot (the notch, Home, Settings) hears about a new pick */
const listeners = new Set<(id: MascotId) => void>();

/** the chosen skin, kept in localStorage */
export function useMascot() {
  const [id, setId] = useState<MascotId>(() => {
    const saved = store.get(KEY) as MascotId | null;
    return saved && (saved === "none" || saved in ART) ? saved : DEFAULT;
  });
  useEffect(() => {
    listeners.add(setId);
    return () => {
      listeners.delete(setId);
    };
  }, []);
  const set = (next: MascotId) => {
    store.set(KEY, next);
    listeners.forEach((l) => l(next));
  };
  return [id, set] as const;
}

/**
 * The mascot hanging under the collapsed notch while an agent works: it comes
 * out from behind the notch's lower edge, dangles, and pulls back up when the
 * work is done (or the notch opens). Each skin has its own way in and its own
 * idle motion (notch.css, `.skin-<id>`): `drop` hides it and pulls it back,
 * `pop` is the entrance, `swing` the dangling.
 */
export function NotchMascot({ id, out, status }: { id: MascotId; out: boolean; status: Status }) {
  if (id === "none") return null;
  return (
    <div className={`notch-mascot skin-${id} ${out ? "is-out" : ""}`} aria-hidden>
      <div className="notch-mascot-drop">
        <div className="notch-mascot-pop">
          <div className="notch-mascot-swing">
            <Mascot id={id} status={status} />
          </div>
        </div>
      </div>
    </div>
  );
}

/** The mascot in its little screen tile; nothing when the skin is "none". */
export function Mascot({ id, status = "idle", className = "" }: { id: MascotId; status?: Status; className?: string }) {
  if (id === "none") return null;
  return (
    <span className={`mascot is-${status} ${className}`} aria-hidden>
      <svg viewBox="0 0 240 220">{ART[id]}</svg>
    </span>
  );
}

/** Settings: one tile per skin plus "None"; the chosen one (and the hovered one) moves. */
export function MascotPicker({ value, onChange }: { value: MascotId; onChange: (id: MascotId) => void }) {
  return (
    <div className="mascot-picker" role="radiogroup" aria-label="Mascot">
      {MASCOTS.map((m) => (
        <button
          key={m.id}
          className={`mascot-pick ${value === m.id ? "on" : ""}`}
          role="radio"
          aria-checked={value === m.id}
          title={m.id === "none" ? "No mascot" : `${m.name}, ${m.what}`}
          onClick={() => onChange(m.id)}
        >
          {m.id === "none" ? (
            <span className="mascot mascot-none">
              <i />
            </span>
          ) : (
            <Mascot id={m.id} />
          )}
        </button>
      ))}
    </div>
  );
}

export const MASCOTS: { id: MascotId; name: string; what: string }[] = [
  { id: "none", name: "None", what: "no mascot" },
  { id: "notchy", name: "Notchy", what: "the island itself" },
  { id: "bitwing", name: "Bitwing", what: "the bat" },
  { id: "hoot", name: "Hoot", what: "the night owl" },
  { id: "fen", name: "Fen", what: "the fennec fox" },
  { id: "jelly", name: "Jelly", what: "the jellyfish" },
  { id: "purr", name: "Purr", what: "the notch cat" },
  { id: "prompt", name: "Prompt", what: "the terminal bot" },
  { id: "whisper", name: "Whisper", what: "the ghost" },
  { id: "slo", name: "Slo", what: "the sloth" },
  { id: "echo", name: "Echo", what: "the parrot" },
];

const ART: Record<Exclude<MascotId, "none">, ReactNode> = {
  notchy: (
    <g className="sway">
      <g className="kick">
        <path d="M100 88 V124" stroke="#000" strokeWidth="7" strokeLinecap="round" />
        <ellipse cx="96" cy="128" rx="10" ry="6" className="acc" />
      </g>
      <g className="kick alt">
        <path d="M140 88 V124" stroke="#000" strokeWidth="7" strokeLinecap="round" />
        <ellipse cx="144" cy="128" rx="10" ry="6" className="acc" />
      </g>
      <path d="M40 0 H200 V56 A36 36 0 0 1 164 92 H76 A36 36 0 0 1 40 56 Z" fill="#000" stroke="rgba(255,255,255,0.08)" />
      <path d="M72 97 H168" className="acc-s glow" strokeWidth="3" strokeLinecap="round" />
      <g className="blink">
        <rect x="86" y="26" width="17" height="26" rx="8.5" fill="#f5f6f8" />
        <rect x="137" y="26" width="17" height="26" rx="8.5" fill="#f5f6f8" />
      </g>
      <circle cx="74" cy="60" r="7" className="acc" opacity="0.45" />
      <circle cx="166" cy="60" r="7" className="acc" opacity="0.45" />
      <rect x="107" y="60" width="6" height="16" rx="3" fill="#f5f6f8" className="eq" />
      <rect x="117" y="56" width="6" height="24" rx="3" fill="#f5f6f8" className="eq d2" />
      <rect x="127" y="60" width="6" height="16" rx="3" fill="#f5f6f8" className="eq d4" />
    </g>
  ),

  bitwing: (
    <>
      <rect x="70" y="0" width="100" height="14" fill="#000" />
      <g className="sway">
        <g transform="rotate(180 120 110)">
          <path d="M111 186 V222 M129 186 V222" stroke="#2e2a38" strokeWidth="5" strokeLinecap="round" />
          <path d="M105 222 q6 6 12 0 M123 222 q6 6 12 0" stroke="#2e2a38" strokeWidth="4" fill="none" strokeLinecap="round" />
          <ellipse cx="120" cy="140" rx="52" ry="56" fill="#24212c" />
          <path d="M120 112 V194 M120 194 Q96 186 74 160 M120 194 Q144 186 166 160" stroke="#17151d" strokeWidth="3" fill="none" />
          <path d="M86 74 L76 28 L110 60 Z" fill="#2e2a38" />
          <path d="M154 74 L164 28 L130 60 Z" fill="#2e2a38" />
          <path d="M89 66 L83 40 L103 58 Z" fill="#d97757" opacity="0.55" />
          <path d="M151 66 L157 40 L137 58 Z" fill="#d97757" opacity="0.55" />
          <circle cx="120" cy="92" r="38" fill="#2e2a38" />
          <g className="blink">
            <circle cx="105" cy="88" r="10" className="acc" />
            <circle cx="135" cy="88" r="10" className="acc" />
          </g>
          <circle cx="108" cy="85" r="3.2" fill="#fff" />
          <circle cx="138" cy="85" r="3.2" fill="#fff" />
          <path d="M110 106 Q115 111 120 106 Q125 111 130 106" stroke="#f5f6f8" strokeWidth="2.5" fill="none" strokeLinecap="round" />
          <path d="M112.5 107 L114.5 114 L117 107.5 Z M127.5 107 L125.5 114 L123 107.5 Z" fill="#fff" />
          <circle cx="96" cy="102" r="5" fill="#d97757" opacity="0.4" />
          <circle cx="144" cy="102" r="5" fill="#d97757" opacity="0.4" />
        </g>
        <path d="M66 176 q-9 10 0 20" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" />
        <path d="M56 170 q-14 16 0 32" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.4s" }} />
        <path d="M174 176 q9 10 0 20" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.2s" }} />
        <path d="M184 170 q14 16 0 32" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.6s" }} />
      </g>
    </>
  ),

  hoot: (
    <>
      <g className="bob">
        <path d="M72 72 L62 34 L96 58 Z" fill="#262833" />
        <path d="M168 72 L178 34 L144 58 Z" fill="#262833" />
        <path d="M60 122 Q48 168 84 194 Q70 152 74 118 Z" fill="#1c1e27" />
        <path d="M180 122 Q192 168 156 194 Q170 152 166 118 Z" fill="#1c1e27" />
        <ellipse cx="120" cy="130" rx="60" ry="70" fill="#262833" />
        <ellipse cx="120" cy="158" rx="36" ry="38" fill="#343746" />
        <g stroke="#555a70" strokeWidth="2.5" fill="none" strokeLinecap="round" strokeLinejoin="round">
          <path d="M100 146 l6 5 -6 5" />
          <path d="M116 146 l6 5 -6 5" />
          <path d="M132 146 l6 5 -6 5" />
          <path d="M108 164 l6 5 -6 5" />
          <path d="M124 164 l6 5 -6 5" />
          <path d="M116 182 l6 5 -6 5" />
        </g>
        <circle cx="98" cy="106" r="25" fill="#f5f6f8" />
        <circle cx="142" cy="106" r="25" fill="#f5f6f8" />
        <g className="blink">
          <circle cx="100" cy="107" r="12" className="acc" />
          <circle cx="140" cy="107" r="12" className="acc" />
          <circle cx="100" cy="107" r="6" fill="#0b0c0e" />
          <circle cx="140" cy="107" r="6" fill="#0b0c0e" />
          <circle cx="103" cy="103" r="3" fill="#fff" />
          <circle cx="143" cy="103" r="3" fill="#fff" />
        </g>
        <path d="M113 126 H127 L120 139 Z" fill="#d97757" />
        <path d="M58 116 C58 26 182 26 182 116" stroke="#c8ccd2" strokeWidth="7" fill="none" strokeLinecap="round" />
        <rect x="46" y="98" width="20" height="38" rx="9" fill="#0b0c0e" />
        <rect x="174" y="98" width="20" height="38" rx="9" fill="#0b0c0e" />
        <rect x="52" y="106" width="8" height="22" rx="4" className="acc glow" />
        <rect x="180" y="106" width="8" height="22" rx="4" className="acc glow" />
        <path d="M104 198 l-4 8 M110 199 v9 M116 198 l4 8 M124 198 l-4 8 M130 199 v9 M136 198 l4 8" stroke="#d97757" strokeWidth="3" strokeLinecap="round" />
      </g>
      <rect x="44" y="204" width="152" height="12" rx="6" fill="#000" />
    </>
  ),

  fen: (
    <>
      <path d="M84 148 Q74 206 82 222 H158 Q166 206 156 148 Q120 162 84 148 Z" fill="#0b0c0e" stroke="rgba(255,255,255,0.08)" />
      <circle cx="102" cy="186" r="5" className="acc glow" />
      <path d="M112 156 v22 M128 156 v22" stroke="#c8ccd2" strokeWidth="2" strokeLinecap="round" />
      <g className="ear-l">
        <path d="M80 104 L40 16 L114 74 Z" fill="#d97757" />
        <path d="M82 94 L54 34 L106 74 Z" fill="#f2b8a0" />
      </g>
      <g className="ear-r">
        <path d="M160 104 L200 16 L126 74 Z" fill="#d97757" />
        <path d="M158 94 L186 34 L134 74 Z" fill="#f2b8a0" />
      </g>
      <ellipse cx="120" cy="112" rx="52" ry="44" fill="#d97757" />
      <path d="M120 152 C92 152 74 132 78 112 C92 122 108 120 120 108 C132 120 148 122 162 112 C166 132 148 152 120 152 Z" fill="#f7e3d4" />
      <g className="blink">
        <ellipse cx="100" cy="108" rx="6.5" ry="8.5" fill="#121216" />
        <ellipse cx="140" cy="108" rx="6.5" ry="8.5" fill="#121216" />
      </g>
      <circle cx="102.5" cy="105" r="2.4" fill="#fff" />
      <circle cx="142.5" cy="105" r="2.4" fill="#fff" />
      <ellipse cx="120" cy="131" rx="6.5" ry="4.5" fill="#121216" />
      <path d="M113 139 Q116.5 143 120 139 Q123.5 143 127 139" stroke="#121216" strokeWidth="2.2" fill="none" strokeLinecap="round" />
      <circle cx="90" cy="126" r="5" fill="#f08a7a" opacity="0.55" />
      <circle cx="150" cy="126" r="5" fill="#f08a7a" opacity="0.55" />
      <path d="M30 40 q-8 10 0 20" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" />
      <path d="M20 34 q-12 16 0 32" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.4s" }} />
      <path d="M210 40 q8 10 0 20" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.2s" }} />
      <path d="M220 34 q12 16 0 32" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.6s" }} />
    </>
  ),

  jelly: (
    <>
      <defs>
        <radialGradient id="mascot-jelly-bell" cx="0.45" cy="0.3" r="0.8">
          <stop offset="0" stopColor="#4a4f78" />
          <stop offset="1" stopColor="#1e2033" />
        </radialGradient>
      </defs>
      <circle cx="62" cy="190" r="4" fill="none" stroke="rgba(255,255,255,0.4)" className="bubble" />
      <circle cx="186" cy="176" r="3" fill="none" stroke="rgba(255,255,255,0.4)" className="bubble b2" />
      <circle cx="196" cy="206" r="5" fill="none" stroke="rgba(255,255,255,0.4)" className="bubble b3" />
      <g className="float">
        <rect x="71" y="96" width="9" height="34" rx="4.5" className="acc eq top" opacity="0.85" />
        <rect x="87" y="96" width="9" height="62" rx="4.5" className="acc eq top d1" opacity="0.85" />
        <rect x="103" y="96" width="9" height="86" rx="4.5" className="acc eq top d2" opacity="0.85" />
        <rect x="119" y="96" width="9" height="70" rx="4.5" className="acc eq top d3" opacity="0.85" />
        <rect x="135" y="96" width="9" height="50" rx="4.5" className="acc eq top d4" opacity="0.85" />
        <rect x="151" y="96" width="9" height="30" rx="4.5" className="acc eq top d5" opacity="0.85" />
        <path
          d="M58 100 C58 34 182 34 182 100 Q172 109 162 100 Q152 109 141 100 Q130 109 120 100 Q110 109 99 100 Q88 109 78 100 Q68 109 58 100 Z"
          fill="url(#mascot-jelly-bell)"
          className="acc-s"
          strokeWidth="2"
          strokeOpacity="0.6"
        />
        <path d="M80 62 Q92 46 112 44" stroke="rgba(255,255,255,0.35)" strokeWidth="4" fill="none" strokeLinecap="round" />
        <g className="blink">
          <circle cx="103" cy="76" r="6" fill="#f5f6f8" />
          <circle cx="137" cy="76" r="6" fill="#f5f6f8" />
        </g>
        <path d="M113 85 Q120 91 127 85" stroke="#f5f6f8" strokeWidth="2.5" fill="none" strokeLinecap="round" />
        <circle cx="92" cy="86" r="5" fill="#f472b6" opacity="0.45" />
        <circle cx="148" cy="86" r="5" fill="#f472b6" opacity="0.45" />
      </g>
    </>
  ),

  purr: (
    <>
      <g className="tail">
        <path d="M152 200 q10 -18 20 0 t20 0 t20 0" stroke="#0a0a0c" strokeWidth="10" fill="none" strokeLinecap="round" />
        <circle cx="212" cy="200" r="5.5" className="acc" />
      </g>
      <path d="M86 116 Q68 200 92 208 H148 Q172 200 154 116 Z" fill="#0a0a0c" stroke="rgba(255,255,255,0.08)" />
      <ellipse cx="106" cy="207" rx="13" ry="7" fill="#0a0a0c" stroke="rgba(255,255,255,0.1)" />
      <ellipse cx="134" cy="207" rx="13" ry="7" fill="#0a0a0c" stroke="rgba(255,255,255,0.1)" />
      <path d="M78 70 L84 26 L112 54 Z" fill="#0a0a0c" stroke="rgba(255,255,255,0.08)" />
      <path d="M162 70 L156 26 L128 54 Z" fill="#0a0a0c" stroke="rgba(255,255,255,0.08)" />
      <path d="M84 60 L87 38 L102 54 Z M156 60 L153 38 L138 54 Z" fill="#2a2c31" />
      <rect x="70" y="46" width="100" height="78" rx="36" fill="#0a0a0c" stroke="rgba(255,255,255,0.08)" />
      <path d="M94 126 H146" className="acc-s" strokeWidth="5" strokeLinecap="round" />
      <circle cx="120" cy="132" r="5" className="acc" />
      <g className="blink">
        <ellipse cx="101" cy="84" rx="11" ry="12" className="acc" />
        <ellipse cx="139" cy="84" rx="11" ry="12" className="acc" />
        <ellipse cx="101" cy="84" rx="2.8" ry="9" fill="#000" />
        <ellipse cx="139" cy="84" rx="2.8" ry="9" fill="#000" />
        <circle cx="104.5" cy="79" r="2.4" fill="#fff" />
        <circle cx="142.5" cy="79" r="2.4" fill="#fff" />
      </g>
      <path d="M116 100 H124 L120 105 Z" fill="#f472b6" />
      <path d="M113 108 Q116.5 112 120 108 Q123.5 112 127 108" stroke="#6b7079" strokeWidth="2" fill="none" strokeLinecap="round" />
      <path d="M64 96 L90 100 M62 106 L90 104 M176 96 L150 100 M178 106 L150 104" stroke="#6b7079" strokeWidth="1.5" strokeLinecap="round" />
    </>
  ),

  prompt: (
    <g className="bob">
      <path d="M120 24 V42" stroke="#9398a1" strokeWidth="3" />
      <circle cx="120" cy="20" r="11" className="acc" opacity="0.25" />
      <circle cx="120" cy="20" r="6.5" className="acc glow" />
      <rect x="52" y="68" width="10" height="30" rx="4" fill="#3a3d46" />
      <rect x="178" y="68" width="10" height="30" rx="4" fill="#3a3d46" />
      <rect x="60" y="42" width="120" height="86" rx="22" fill="#1d1f25" stroke="#3a3d46" strokeWidth="2" />
      <rect x="72" y="54" width="96" height="62" rx="13" fill="#050608" />
      <path d="M88 68 l11 9 -11 9" className="acc-s" strokeWidth="4.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="128" y="82" width="22" height="5" rx="2.5" className="acc cursor" />
      <rect x="104" y="98" width="4" height="10" rx="2" fill="#f5f6f8" className="eq" />
      <rect x="111" y="96" width="4" height="14" rx="2" fill="#f5f6f8" className="eq d2" />
      <rect x="118" y="94" width="4" height="18" rx="2" fill="#f5f6f8" className="eq d4" />
      <rect x="125" y="96" width="4" height="14" rx="2" fill="#f5f6f8" className="eq d1" />
      <rect x="132" y="98" width="4" height="10" rx="2" fill="#f5f6f8" className="eq d3" />
      <path d="M84 146 Q62 158 64 180" stroke="#3a3d46" strokeWidth="9" fill="none" strokeLinecap="round" />
      <circle cx="64" cy="184" r="8" fill="#d97757" />
      <g className="wavearm">
        <path d="M156 146 Q182 136 186 112" stroke="#3a3d46" strokeWidth="9" fill="none" strokeLinecap="round" />
        <circle cx="186" cy="106" r="8" fill="#d97757" />
      </g>
      <rect x="82" y="134" width="76" height="56" rx="16" fill="#1d1f25" stroke="#3a3d46" strokeWidth="2" />
      <rect x="100" y="148" width="40" height="13" rx="6.5" fill="#000" />
      <circle cx="109" cy="154.5" r="3.5" className="acc" />
      <rect x="94" y="190" width="16" height="16" rx="5" fill="#3a3d46" />
      <rect x="130" y="190" width="16" height="16" rx="5" fill="#3a3d46" />
      <rect x="88" y="204" width="26" height="9" rx="4.5" fill="#d97757" />
      <rect x="126" y="204" width="26" height="9" rx="4.5" fill="#d97757" />
    </g>
  ),

  whisper: (
    <>
      <ellipse cx="114" cy="208" rx="36" ry="5" fill="#000" opacity="0.35" />
      <g className="float">
        <path
          d="M68 186 V110 C68 56 160 56 160 110 V186 Q151 174 141 186 Q132 198 123 186 Q114 174 105 186 Q96 198 87 186 Q78 174 68 186 Z"
          fill="#eef0f6"
        />
        <path d="M146 92 C152 104 152 150 150 176" stroke="#d6d9e4" strokeWidth="10" fill="none" strokeLinecap="round" opacity="0.7" />
        <g className="blink">
          <ellipse cx="98" cy="110" rx="6" ry="9" fill="#15161a" />
          <ellipse cx="128" cy="110" rx="6" ry="9" fill="#15161a" />
        </g>
        <circle cx="100" cy="106" r="2.2" fill="#fff" />
        <circle cx="130" cy="106" r="2.2" fill="#fff" />
        <circle cx="86" cy="124" r="6" fill="#f472b6" opacity="0.4" />
        <circle cx="140" cy="124" r="6" fill="#f472b6" opacity="0.4" />
        <ellipse cx="114" cy="130" rx="4.5" ry="5.5" fill="#15161a" />
        <ellipse cx="160" cy="132" rx="9" ry="14" fill="#eef0f6" transform="rotate(-20 160 132)" />
        <path d="M180 116 q7 9 0 18" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" />
        <path d="M190 110 q11 15 0 30" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.3s" }} />
        <path d="M200 104 q15 21 0 42" className="acc-s wave" strokeWidth="3" fill="none" strokeLinecap="round" style={{ animationDelay: "0.6s" }} />
      </g>
    </>
  ),

  slo: (
    <>
      <path d="M60 0 H180 V10 A20 20 0 0 1 160 30 H80 A20 20 0 0 1 60 10 Z" fill="#000" />
      <path d="M84 30 H156" className="acc-s glow" strokeWidth="2.5" strokeLinecap="round" />
      <g className="sway slow">
        <path d="M94 24 Q86 56 98 86" stroke="#7a5c48" strokeWidth="17" fill="none" strokeLinecap="round" />
        <path d="M146 24 Q154 56 142 86" stroke="#7a5c48" strokeWidth="17" fill="none" strokeLinecap="round" />
        <path d="M88 20 q2 -8 6 -10 M95 19 q2 -8 6 -9 M140 19 q2 -8 6 -9 M147 20 q2 -8 6 -10" stroke="#f1e1cf" strokeWidth="2.5" fill="none" strokeLinecap="round" />
        <ellipse cx="120" cy="156" rx="42" ry="46" fill="#8a6a55" />
        <path d="M98 194 Q92 210 100 216 M142 194 Q148 210 140 216" stroke="#7a5c48" strokeWidth="14" fill="none" strokeLinecap="round" />
        <ellipse cx="120" cy="164" rx="24" ry="28" fill="#a3846c" />
        <circle cx="120" cy="104" r="38" fill="#9a7a62" />
        <ellipse cx="120" cy="110" rx="30" ry="23" fill="#f1e1cf" />
        <ellipse cx="104" cy="106" rx="11" ry="6.5" fill="#4a3427" transform="rotate(-22 104 106)" />
        <ellipse cx="136" cy="106" rx="11" ry="6.5" fill="#4a3427" transform="rotate(22 136 106)" />
        <path d="M100 107 Q104 104 108 107" stroke="#f1e1cf" strokeWidth="2.2" fill="none" strokeLinecap="round" />
        <path d="M132 107 Q136 104 140 107" stroke="#f1e1cf" strokeWidth="2.2" fill="none" strokeLinecap="round" />
        <ellipse cx="120" cy="117" rx="5.5" ry="3.8" fill="#3a2a20" />
        <path d="M111 123 Q120 131 129 123" stroke="#3a2a20" strokeWidth="2.2" fill="none" strokeLinecap="round" />
        <circle cx="120" cy="164" r="6" className="acc glow" />
      </g>
    </>
  ),

  echo: (
    <>
      <rect x="160" y="24" width="68" height="40" rx="13" fill="#f5f6f8" />
      <path d="M172 62 L166 76 L184 63 Z" fill="#f5f6f8" />
      <rect x="171" y="33" width="46" height="5" rx="2.5" fill="#2a2c31" className="type" />
      <rect x="171" y="42" width="38" height="5" rx="2.5" fill="#2a2c31" className="type t2" />
      <rect x="171" y="51" width="28" height="5" rx="2.5" className="acc type t3" />
      <g className="bob">
        <path d="M106 176 L90 214 L112 210 L122 180 Z" fill="#6366f1" />
        <path d="M112 178 L104 214 L118 208 Z" fill="#818cf8" />
        <ellipse cx="120" cy="138" rx="34" ry="46" fill="#d97757" />
        <path d="M100 116 Q84 158 110 186 Q126 158 118 116 Z" fill="#b85f42" />
        <path d="M102 168 Q104 180 110 186 Q112 174 108 166 Z" fill="#6366f1" />
        <path d="M118 62 Q108 42 122 32 Q122 48 130 58 Z" fill="#f0956f" />
        <path d="M128 60 Q126 40 142 36 Q136 50 138 60 Z" fill="#f0956f" />
        <circle cx="128" cy="88" r="30" fill="#d97757" />
        <ellipse cx="140" cy="88" rx="14" ry="16" fill="#fff4ec" />
        <g className="blink">
          <circle cx="141" cy="84" r="5.2" fill="#121216" />
        </g>
        <circle cx="142.8" cy="82.2" r="1.8" fill="#fff" />
        <path d="M152 82 Q174 80 170 106 Q162 96 150 98 Z" fill="#2a2c31" />
        <path d="M151 98 Q160 98 162 104 Q154 108 150 102 Z" fill="#3a3d46" />
      </g>
      <path d="M108 186 v8 M114 186 v8 M126 186 v8 M132 186 v8" stroke="#9398a1" strokeWidth="3.5" strokeLinecap="round" />
      <rect x="40" y="190" width="160" height="22" rx="11" fill="#000" stroke="rgba(255,255,255,0.08)" />
      <circle cx="56" cy="201" r="4.5" className="acc glow" />
      <rect x="170" y="197" width="4" height="8" rx="2" fill="#f5f6f8" className="eq" />
      <rect x="177" y="195" width="4" height="12" rx="2" fill="#f5f6f8" className="eq d2" />
      <rect x="184" y="197" width="4" height="8" rx="2" fill="#f5f6f8" className="eq d4" />
    </>
  ),
};
