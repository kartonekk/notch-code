import type { CSSProperties } from "react";

/** fixed pseudo-random layout, so embers don't jump around on re-render */
const EMBERS = [
  { x: 6, d: 3.4, delay: 0, s: 3 },
  { x: 17, d: 4.6, delay: -1.2, s: 2 },
  { x: 29, d: 3.1, delay: -2.4, s: 3 },
  { x: 41, d: 5.2, delay: -0.6, s: 2 },
  { x: 52, d: 3.8, delay: -3.1, s: 4 },
  { x: 63, d: 4.2, delay: -1.8, s: 2 },
  { x: 74, d: 3.3, delay: -0.3, s: 3 },
  { x: 85, d: 4.9, delay: -2.7, s: 2 },
  { x: 94, d: 3.6, delay: -1.5, s: 3 },
];

const vars = (v: Record<string, string | number>) => v as CSSProperties;

/** Hazard tape above the composer while reckless mode is on. */
export function RecklessStrip({ running }: { running: boolean }) {
  return (
    <div className={`reckless-strip ${running ? "is-fast" : ""}`}>
      <span className="hazard" />
      <span className="reckless-strip-text">
        <b>No brakes</b> — Claude runs anything, every change is kept
      </span>
      <span className="hazard is-right" />
    </div>
  );
}

/** Embers rising through the expanded panel. */
export function Embers({ fast }: { fast: boolean }) {
  return (
    <div className={`embers ${fast ? "is-fast" : ""}`} aria-hidden>
      {EMBERS.map((e, i) => (
        <i key={i} style={vars({ left: `${e.x}%`, width: e.s, height: e.s, "--d": `${e.d}s`, animationDelay: `${e.delay}s` })} />
      ))}
    </div>
  );
}
