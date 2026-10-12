import type { ReactNode } from "react";
import { api, ClaudeStatus, StepIcon, UpdateStatus } from "./api";

// ---- helpers ------------------------------------------------------------------

export function formatDuration(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(sec).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** "34 s", "2 min 5 s" */
export function formatSpan(ms: number) {
  const s = Math.max(1, Math.round(ms / 1000));
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)} min ${s % 60} s`;
}

export function basename(path: string) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function capitalize(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** localStorage that never throws (private windows, blocked storage) */
export const store = {
  get(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* not persisted, fine */
    }
  },
};

// ---- small components -----------------------------------------------------------

export function IconButton(props: { title: string; onClick: () => void; active?: boolean; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      className={`icon-btn ${props.active ? "is-active" : ""}`}
      title={props.title}
      aria-label={props.title}
      disabled={props.disabled}
      onClick={props.onClick}
    >
      {props.children}
    </button>
  );
}

export function Bars({ level }: { level: number }) {
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

export function Meter({ level }: { level: number }) {
  const l = Math.min(1, Math.sqrt(level) * 1.4);
  return (
    <span className="meter">
      <i style={{ width: `${l * 100}%` }} />
    </span>
  );
}

// ---- banners --------------------------------------------------------------------

export function needsClaudeAttention(c: ClaudeStatus | null) {
  return !!c && (c.state === "signed_out" || c.state === "not_installed" || c.state === "error");
}

/** Reminder shown when Claude has to be signed in again (or installed). */
export function ClaudeBanner({ claude, onLater }: { claude: ClaudeStatus; onLater: () => void }) {
  const working = claude.state === "signing_in" || claude.state === "installing";
  const canSignIn = claude.state === "signed_out" || claude.state === "error";
  return (
    <div className={`banner ${working ? "is-info" : "is-warn"}`}>
      {working ? <span className="mini-spinner" /> : <KeyIcon />}
      <span className="banner-text" title={claude.message ?? undefined}>
        {claude.message ?? "Claude needs attention"}
      </span>
      {!working && (
        <button className="btn btn-ghost btn-sm" onClick={onLater}>
          Later
        </button>
      )}
      {canSignIn && claude.suggest_window && (
        <button className="btn btn-ghost btn-sm" onClick={() => api.claudeLogin(true)}>
          Sign-in window
        </button>
      )}
      {canSignIn && (
        <button className="btn btn-accent btn-sm" onClick={() => api.claudeLogin(false)}>
          Sign in
        </button>
      )}
      {claude.state === "not_installed" && (
        <button className="btn btn-accent btn-sm" onClick={() => api.claudeInstall()}>
          Install
        </button>
      )}
    </div>
  );
}

/** Offers a newer build; nothing is downloaded until the user clicks Update. */
export function UpdateBanner({ update, recording, onLater }: { update: UpdateStatus; recording: boolean; onLater: () => void }) {
  const downloading = update.state === "downloading";
  const text = downloading
    ? `Downloading update… ${Math.round((update.progress ?? 0) * 100)}%`
    : update.state === "error"
      ? `Update failed: ${update.message ?? "unknown error"}`
      : update.current == null
        ? `Build ${update.available} available (you're on a local build)`
        : `Update available — build ${update.available}`;
  return (
    <div className={`banner ${update.state === "error" ? "is-error" : "is-info"}`}>
      {downloading ? <span className="mini-spinner" /> : <DownloadIcon />}
      <span className="banner-text" title={text}>
        {text}
      </span>
      {!downloading && (
        <>
          <button className="btn btn-ghost btn-sm" onClick={onLater}>
            Later
          </button>
          <button
            className="btn btn-accent btn-sm"
            disabled={recording}
            title={recording ? "Finish the recording first" : "Download, install and restart"}
            onClick={() => api.updateInstall().catch(() => {})}
          >
            {update.state === "error" ? "Try again" : "Update"}
          </button>
        </>
      )}
    </div>
  );
}

// ---- icons ----------------------------------------------------------------------

const svg = { width: 15, height: 15, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round" } as const;
const small = { ...svg, width: 13, height: 13 };

export const PinIcon = () => (
  <svg {...svg}>
    <path d="M12 17v5M9 3h6l-1 7 4 3v2H6v-2l4-3-1-7z" />
  </svg>
);
export const FolderIcon = ({ size = 15 }: { size?: number }) => (
  <svg {...svg} width={size} height={size}>
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
  </svg>
);
export const PowerIcon = () => (
  <svg {...svg}>
    <path d="M12 3v9M6.3 6.3a8 8 0 1 0 11.4 0" />
  </svg>
);
export const PauseIcon = () => (
  <svg {...small}>
    <path d="M8 5v14M16 5v14" />
  </svg>
);
export const PlayIcon = () => (
  <svg {...small}>
    <path d="M7 5l12 7-12 7z" />
  </svg>
);
export const KeyIcon = () => (
  <svg {...small}>
    <circle cx="7.5" cy="15.5" r="5.5" />
    <path d="M21 2l-9.6 9.6M15.5 7.5l3 3L22 7l-3-3" />
  </svg>
);
export const DownloadIcon = () => (
  <svg {...small}>
    <path d="M12 3v12M7 10l5 5 5-5M5 21h14" />
  </svg>
);
export const TerminalIcon = () => (
  <svg {...small}>
    <rect x="2" y="4" width="20" height="16" rx="2" />
    <path d="M6 9l3 3-3 3M12 15h5" />
  </svg>
);
export const CodeIcon = () => (
  <svg {...small}>
    <path d="M8 6l-6 6 6 6M16 6l6 6-6 6" />
  </svg>
);
export const MicIcon = () => (
  <svg {...small}>
    <rect x="9" y="2" width="6" height="12" rx="3" />
    <path d="M5 10a7 7 0 0 0 14 0M12 17v5" />
  </svg>
);
/** voice prompts switched off */
export const MicOffIcon = () => (
  <svg {...small}>
    <path d="M15 9.3V5a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.8 2.4" />
    <path d="M19 10a7 7 0 0 1-1.2 3.9M5 10a7 7 0 0 0 10.9 5.8M12 17v5M3 3l18 18" />
  </svg>
);
export const ImageIcon = () => (
  <svg {...small}>
    <rect x="3" y="3" width="18" height="18" rx="2" />
    <circle cx="9" cy="9" r="2" />
    <path d="M21 15l-5-5L5 21" />
  </svg>
);
export const SendIcon = () => (
  <svg {...svg}>
    <path d="M12 19V5M5 12l7-7 7 7" />
  </svg>
);
export const PlusIcon = () => (
  <svg {...svg}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);
export const ChevronIcon = ({ open }: { open?: boolean }) => (
  <svg {...small} width={11} height={11} style={{ transform: open ? "rotate(180deg)" : undefined, transition: "transform 150ms" }}>
    <path d="M6 9l6 6 6-6" />
  </svg>
);
export const CheckIcon = () => (
  <svg {...small}>
    <path d="M5 12l5 5 9-10" />
  </svg>
);
export const CrossIcon = () => (
  <svg {...small}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);
export const FlameIcon = () => (
  <svg {...small}>
    <path d="M12 22c4 0 7-2.7 7-6.8 0-3.3-2-5.6-3.6-7.4-.4 1.9-1.4 3-2.6 3.4.5-3.4-.9-6.4-3.8-9.2-.2 3.6-2.3 5.7-3.8 7.6C4 11.1 5 14.3 5 15.2 5 19.3 8 22 12 22z" />
  </svg>
);
export const CopyIcon = () => (
  <svg {...small}>
    <rect x="9" y="9" width="12" height="12" rx="2" />
    <path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" />
  </svg>
);
export const RetryIcon = () => (
  <svg {...small} width={11} height={11}>
    <path d="M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5" />
  </svg>
);
export const AlertIcon = () => (
  <svg {...small}>
    <path d="M12 3l10 18H2zM12 10v5M12 18v.5" />
  </svg>
);

const stepPaths: Record<StepIcon, ReactNode> = {
  read: <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z" />,
  create: <path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9zM14 3v6h6M12 12v6M9 15h6" />,
  edit: <path d="M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4" />,
  run: <path d="M4 17l6-5-6-5M12 19h8" />,
  search: <path d="M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4" />,
  web: <path d="M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3c2.5 2.7 2.5 15.3 0 18M12 3c-2.5 2.7-2.5 15.3 0 18" />,
  plan: <path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01" />,
  agent: <path d="M12 3l2.2 5.8L20 11l-5.8 2.2L12 19l-2.2-5.8L4 11l5.8-2.2z" />,
  other: <path d="M12 12h.01M7 12h.01M17 12h.01" />,
};

export const StepGlyph = ({ icon }: { icon: StepIcon }) => <svg {...small}>{stepPaths[icon] ?? stepPaths.other}</svg>;
