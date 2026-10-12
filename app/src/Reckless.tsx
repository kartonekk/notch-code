/**
 * Reckless mode in the collapsed notch: a little car racing to the right,
 * road dashes flying past, exhaust smoke puffing out behind it and the words
 * drifting in the smoke. `fast` = Claude is working (everything speeds up).
 */
export function RecklessRoad({ text, fast }: { text: string; fast: boolean }) {
  return (
    <div className={`road ${fast ? "is-fast" : ""}`} aria-label={text}>
      <div className="road-dashes" />
      <div className="smoke">
        {Array.from({ length: 7 }, (_, i) => (
          <i key={i} style={{ animationDelay: `${i * -0.22}s` }} />
        ))}
      </div>
      <span className="smoke-text" key={text}>
        {text}
      </span>
      <Car />
    </div>
  );
}

function Car() {
  return (
    <svg className="car" width="46" height="22" viewBox="0 0 46 22" aria-hidden>
      <defs>
        <linearGradient id="car-body" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ff6b4a" />
          <stop offset="1" stopColor="#d7261b" />
        </linearGradient>
      </defs>
      <g className="car-body">
        {/* body */}
        <path d="M3 15.5c0-2 1.2-3.3 3.2-3.6l6.3-.9 5.6-5c.9-.8 2-1.2 3.2-1.2h8.4c1.3 0 2.5.6 3.3 1.6l3.6 4.6 4.4 1c1.7.4 2.9 1.9 2.9 3.6v1.9c0 .8-.6 1.5-1.4 1.5H4.4C3.6 19 3 18.3 3 17.5z" fill="url(#car-body)" />
        {/* windows */}
        <path d="M15.4 11.2l4-3.6c.5-.4 1.1-.6 1.7-.6h4.1v4.2z" fill="#9fd8ff" opacity=".9" />
        <path d="M27 7h3.3c.8 0 1.5.4 2 1l2.5 3.2H27z" fill="#9fd8ff" opacity=".9" />
        {/* stripe, headlight, exhaust */}
        <path d="M6 14.2h36" stroke="#fff" strokeOpacity=".55" strokeWidth="1.1" />
        <rect x="41.6" y="12.6" width="2.6" height="1.6" rx=".8" fill="#fff6c2" />
        <rect x="1" y="16.4" width="3" height="1.4" rx=".7" fill="#555" />
      </g>
      {/* wheels */}
      <g className="wheel" style={{ transformOrigin: "12px 18.5px" }}>
        <circle cx="12" cy="18.5" r="3.5" fill="#1c1c1e" stroke="#8e8e93" strokeWidth="1.2" />
        <path d="M12 15.6v5.8M9.1 18.5h5.8" stroke="#8e8e93" strokeWidth=".9" />
      </g>
      <g className="wheel" style={{ transformOrigin: "35px 18.5px" }}>
        <circle cx="35" cy="18.5" r="3.5" fill="#1c1c1e" stroke="#8e8e93" strokeWidth="1.2" />
        <path d="M35 15.6v5.8M32.1 18.5h5.8" stroke="#8e8e93" strokeWidth=".9" />
      </g>
    </svg>
  );
}
