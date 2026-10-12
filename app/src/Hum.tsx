import { useEffect, useRef } from "react";
import type { CSSProperties } from "react";

/**
 * What Hum is up to. The body keeps its colour; the mood shows in the face and
 * the ear pads of its headphones.
 */
export type Mood = "idle" | "sleep" | "listen" | "paused" | "work" | "fire" | "voice" | "happy" | "warn" | "error";

/** how Hum looks after something ended well, so-so or badly */
export const toneMood = (tone: "ok" | "warn" | "error"): Mood => (tone === "ok" ? "happy" : tone);

/**
 * Hum, the notch's little character: a wide soft bun wearing headphones whose
 * ear pads take the colour of what's going on (red while it records). Plain SVG
 * with CSS animations (`.hum` in notch.css), so it costs nothing while idle.
 * `size` is the width; Hum is 64:44.
 *
 * `level` (0..1) drives the sound bars on the pads while listening; `hue` gives a session
 * its own colour; `look` makes the eyes follow the pointer.
 */
export function Hum({
  mood,
  size = 36,
  level = 0,
  hue,
  look = false,
  className,
}: {
  mood: Mood;
  size?: number;
  level?: number;
  hue?: number;
  look?: boolean;
  className?: string;
}) {
  const ref = useRef<SVGSVGElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!look || !el) return;
    const onMove = (e: PointerEvent) => {
      const r = el.getBoundingClientRect();
      const dx = e.clientX - (r.left + r.width / 2);
      const dy = e.clientY - (r.top + r.height / 2);
      const d = Math.hypot(dx, dy) || 1;
      const k = Math.min(1, d / 200);
      el.style.setProperty("--lx", `${((dx / d) * k * 2.6).toFixed(2)}px`);
      el.style.setProperty("--ly", `${((dy / d) * k * 1.8).toFixed(2)}px`);
    };
    window.addEventListener("pointermove", onMove);
    return () => {
      window.removeEventListener("pointermove", onMove);
      el.style.removeProperty("--lx");
      el.style.removeProperty("--ly");
    };
  }, [look]);

  const vars: Record<string, string | number> = { "--lvl": Math.min(1, Math.sqrt(level) * 1.6).toFixed(2) };
  if (hue != null) {
    vars["--body"] = `hsl(${hue} 75% 76%)`;
    // sessions don't all blink at the same moment
    vars["--blink-delay"] = `${-(hue % 50) / 10}s`;
  }

  return (
    <svg
      ref={ref}
      className={`hum hum-${mood} ${className ?? ""}`}
      viewBox="0 0 64 44"
      width={size}
      height={Math.round((size * 44) / 64)}
      style={vars as CSSProperties}
      aria-hidden
    >
      <g className="hum-body">
        <path className="hum-band" d="M7.75 21 C7.75 3.5 56.25 3.5 56.25 21" />
        <rect className="hum-shape" x="9" y="13" width="46" height="29" rx="14.5" />
        <ellipse className="hum-shine" cx="19" cy="18" rx="5" ry="2.2" transform="rotate(-16 19 18)" />
        {/* ear cups: dark shell, coloured pad, and sound bars on the pad while it listens */}
        <rect className="hum-shell" x="3" y="20" width="9.5" height="16" rx="4.75" />
        <rect className="hum-shell" x="51.5" y="20" width="9.5" height="16" rx="4.75" />
        <rect className="hum-cup" x="4.6" y="22" width="6.3" height="12" rx="3.15" />
        <rect className="hum-cup" x="53.1" y="22" width="6.3" height="12" rx="3.15" />
        <g className="hum-eq">
          <rect x="5.5" y="25" width="1.2" height="6" rx="0.6" />
          <rect x="7.15" y="25" width="1.2" height="6" rx="0.6" />
          <rect x="8.8" y="25" width="1.2" height="6" rx="0.6" />
          <rect x="54" y="25" width="1.2" height="6" rx="0.6" />
          <rect x="55.65" y="25" width="1.2" height="6" rx="0.6" />
          <rect x="57.3" y="25" width="1.2" height="6" rx="0.6" />
        </g>
        <g className="hum-face">
          <g className="hum-eyes">
            <rect className="hum-eye" x="23" y="24" width="4.4" height="7" rx="2.2" />
            <rect className="hum-eye" x="36.6" y="24" width="4.4" height="7" rx="2.2" />
          </g>
          <path className="hum-line hum-smile" d="M22.8 28.5 q2.4 -3.6 4.8 0 M36.4 28.5 q2.4 -3.6 4.8 0" />
          <path className="hum-line hum-shut" d="M23 27.5 h4.4 M36.6 27.5 h4.4" />
          <ellipse className="hum-mouth" cx="32" cy="35" rx="1.9" ry="1.5" />
          <ellipse className="hum-blush" cx="18" cy="32.5" rx="2.4" ry="1.4" />
          <ellipse className="hum-blush" cx="46" cy="32.5" rx="2.4" ry="1.4" />
          <g className="hum-dots">
            <circle cx="28" cy="36.5" r="1.1" />
            <circle cx="32" cy="36.5" r="1.1" />
            <circle cx="36" cy="36.5" r="1.1" />
          </g>
        </g>
      </g>
    </svg>
  );
}
