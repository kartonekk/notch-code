import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AgentEvent, AgentMode, AgentStatus, api, Denied, Limit, on, StepIcon } from "./api";
import { ChangesBar, ReviewState } from "./Review";
import {
  AlertIcon,
  basename,
  CheckIcon,
  ChevronIcon,
  CrossIcon,
  FlameIcon,
  FolderIcon,
  formatSpan,
  IconButton,
  PlusIcon,
  SendIcon,
  StepGlyph,
  store,
} from "./ui";

// ---- state ----------------------------------------------------------------------

export type FeedItem =
  | { type: "prompt"; run: number; text: string }
  | { type: "text"; run: number; text: string }
  | { type: "step"; run: number; id: string; icon: StepIcon; label: string; detail: string | null; state: "running" | "ok" | "error"; output?: string | null }
  | { type: "done"; run: number; ok: boolean; stopped: boolean; message: string | null; duration_ms: number; denied: Denied[]; steps: number };

/** What the collapsed bar says about a run that ended while the notch was closed. */
export interface AgentNotice {
  tone: "ok" | "warn" | "error";
  text: string;
}

const MAX_ITEMS = 2000;

function applyEvent(prev: FeedItem[], e: AgentEvent): FeedItem[] {
  let next: FeedItem[];
  switch (e.kind) {
    case "start":
      next = [...prev, { type: "prompt", run: e.run, text: e.prompt }];
      break;
    case "text":
      next = [...prev, { type: "text", run: e.run, text: e.text }];
      break;
    case "step":
      next = [...prev, { type: "step", run: e.run, id: e.id, icon: e.icon, label: e.label, detail: e.detail, state: "running" }];
      break;
    case "step_done":
      next = prev.map((i) => (i.type === "step" && i.id === e.id ? { ...i, state: e.ok ? "ok" : "error", output: e.output } : i));
      break;
    case "done": {
      const steps = prev.filter((i) => i.type === "step" && i.run === e.run).length;
      next = [
        ...prev.map((i) => (i.type === "step" && i.run === e.run && i.state === "running" ? { ...i, state: e.ok ? ("ok" as const) : ("error" as const) } : i)),
        { type: "done", run: e.run, ok: e.ok, stopped: e.stopped, message: e.message, duration_ms: e.duration_ms, denied: e.denied, steps },
      ];
      break;
    }
    default:
      return prev;
  }
  return next.length > MAX_ITEMS ? next.slice(-MAX_ITEMS) : next;
}

function noticeFor(e: Extract<AgentEvent, { kind: "done" }>): AgentNotice | null {
  if (e.stopped) return null;
  if (e.denied.length > 0) return { tone: "warn", text: "Claude needs your OK to continue" };
  if (!e.ok) return { tone: "error", text: e.message ?? "Claude hit a problem" };
  return { tone: "ok", text: `Claude finished in ${formatSpan(e.duration_ms)}` };
}

const MODES: { id: AgentMode; label: string; title: string }[] = [
  { id: "auto", label: "Auto", title: "Claude edits files and runs safe commands by itself; risky ones are blocked" },
  { id: "acceptEdits", label: "Edits", title: "Claude edits files; commands need your OK" },
  { id: "plan", label: "Plan", title: "Read-only: Claude looks around and proposes a plan" },
  { id: "reckless", label: "Reckless", title: "No brakes: Claude runs anything it wants and every change is kept automatically" },
];

export const MODELS: { id: string; label: string; title: string }[] = [
  { id: "", label: "Default", title: "Whatever Claude Code uses by default" },
  { id: "fable", label: "Fable", title: "Latest Fable" },
  { id: "opus", label: "Opus", title: "Latest Opus: most capable" },
  { id: "sonnet", label: "Sonnet", title: "Latest Sonnet: fast and capable" },
  { id: "haiku", label: "Haiku", title: "Latest Haiku: fastest, lightest on the limit" },
];

function cachedLimit(): Limit | null {
  try {
    const l = JSON.parse(store.get("agent.limit") ?? "null") as Limit | null;
    return l && (l.resets_at ?? 0) > Date.now() ? l : null;
  } catch {
    return null;
  }
}

export function useAgent(expanded: boolean) {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [items, setItems] = useState<FeedItem[]>([]);
  const [notice, setNotice] = useState<AgentNotice | null>(null);
  const [mode, setModeState] = useState<AgentMode>(() => {
    const m = store.get("agent.mode");
    return m === "plan" || m === "acceptEdits" || m === "reckless" ? m : "auto";
  });
  const [model, setModelState] = useState(() => store.get("agent.model") ?? "");
  const [error, setError] = useState<string | null>(null);
  const expandedRef = useRef(expanded);
  expandedRef.current = expanded;

  useEffect(() => {
    api.agentStatus().then(setStatus).catch(() => {});
    const subs = [
      on<AgentStatus>("agent-status", setStatus),
      on<AgentEvent>("agent", (e) => {
        setItems((prev) => applyEvent(prev, e));
        if (e.kind === "done" && !expandedRef.current) setNotice(noticeFor(e));
      }),
    ];
    return () => subs.forEach((p) => p.then((un) => un()));
  }, []);

  // remember the last known 5-hour window across restarts
  const reported = status?.limit ?? null;
  useEffect(() => {
    if (reported) store.set("agent.limit", JSON.stringify(reported));
  }, [reported]);
  const limit = reported ?? cachedLimit();

  // opening the notch shows the result, so the bar notice has done its job
  useEffect(() => {
    if (expanded) setNotice(null);
  }, [expanded]);

  const call = (p: Promise<unknown>) => {
    setError(null);
    p.catch((e) => setError(String(e)));
  };

  return {
    status,
    items,
    notice,
    mode,
    error,
    running: !!status?.running,
    setMode: (m: AgentMode) => {
      setModeState(m);
      store.set("agent.mode", m);
    },
    model,
    limit,
    setModel: (m: string) => {
      setModelState(m);
      store.set("agent.model", m);
    },
    send: (prompt: string, allow: string[] = []) => call(api.agentSend(prompt, mode, allow, model || null)),
    stop: () => call(api.agentStop()),
    newChat: () => {
      setItems([]);
      call(api.agentNew());
    },
    setProject: (path: string) => {
      setItems([]);
      call(api.agentSetProject(path));
    },
    pickProject: async () => {
      setError(null);
      try {
        const picked = await api.agentPickProject();
        if (picked) setItems([]);
      } catch (e) {
        setError(String(e));
      }
    },
  };
}

export type AgentState = ReturnType<typeof useAgent>;

// ---- tab ------------------------------------------------------------------------

const SUGGESTIONS = [
  "Explain how this project works",
  "Find bugs and fix them",
  "Run the tests and fix what fails",
  "Review my uncommitted changes",
];

export function CodeTab({ agent, review, onHold }: { agent: AgentState; review: ReviewState; onHold: (hold: boolean) => void }) {
  const { status, items, running } = agent;
  const [draft, setDraft] = useState(() => store.get("agent.draft") ?? "");
  const [menuOpen, setMenuOpen] = useState(false);
  const [picking, setPicking] = useState(false);
  const [focused, setFocused] = useState(false);
  const [scrolledUp, setScrolledUp] = useState(false);
  /** Reckless needs a second click */
  const [arming, setArming] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const feedRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const stick = useRef(true);
  const project = status?.project ?? null;

  // keep the notch open while typing, choosing a folder or using the menu
  const hold = menuOpen || modelOpen || picking || focused;
  useEffect(() => onHold(hold), [hold, onHold]);
  useEffect(() => () => onHold(false), [onHold]);

  useEffect(() => store.set("agent.draft", draft), [draft]);

  // autosize the composer (1–5 lines)
  useLayoutEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 110)}px`;
  }, [draft]);

  // newest at the bottom; scrolling up pauses following
  useLayoutEffect(() => {
    const el = feedRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [items]);
  useLayoutEffect(() => {
    stick.current = true;
    setScrolledUp(false);
    const el = feedRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, []);

  const submit = () => {
    const text = draft.trim();
    if (!text || running || !project) return;
    stick.current = true;
    agent.send(text);
    setDraft("");
  };

  const pick = async () => {
    setMenuOpen(false);
    setPicking(true);
    await agent.pickProject();
    setPicking(false);
    inputRef.current?.focus();
  };

  const lastDone = useMemo(() => {
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i];
      if (it.type === "done") return it;
      if (it.type === "prompt") return null;
    }
    return null;
  }, [items]);

  return (
    <div className="code">
      {/* ---- toolbar: project · mode · new chat ---- */}
      <div className="toolbar">
        <div className="project">
          <button
            className={`project-btn ${menuOpen ? "is-open" : ""}`}
            title={project ?? "Choose a project folder"}
            disabled={running}
            onClick={() => (status?.recent.length ? setMenuOpen(!menuOpen) : pick())}
          >
            <FolderIcon size={13} />
            <span className="project-name">{project ? basename(project) : "Choose project…"}</span>
            {(status?.recent.length ?? 0) > 0 && <ChevronIcon open={menuOpen} />}
          </button>
          {menuOpen && (
            <>
              <div className="menu-scrim" onClick={() => setMenuOpen(false)} />
              <div className="menu" role="menu">
                {status?.recent.map((p) => (
                  <button
                    key={p}
                    className={`menu-item ${p === project ? "is-current" : ""}`}
                    role="menuitem"
                    onClick={() => {
                      setMenuOpen(false);
                      if (p !== project) agent.setProject(p);
                    }}
                  >
                    <span className="menu-title">{basename(p)}</span>
                    <span className="menu-sub">{p}</span>
                  </button>
                ))}
                <div className="menu-sep" />
                <button className="menu-item" role="menuitem" onClick={pick}>
                  <span className="menu-title">Choose another folder…</span>
                </button>
                {project && (
                  <button
                    className="menu-item"
                    role="menuitem"
                    onClick={() => {
                      setMenuOpen(false);
                      api.agentOpenProject().catch(() => {});
                    }}
                  >
                    <span className="menu-title">Open in Explorer</span>
                  </button>
                )}
              </div>
            </>
          )}
        </div>

        <div className="segmented" role="radiogroup" aria-label="What Claude may do">
          {MODES.map((m) => (
            <button
              key={m.id}
              role="radio"
              aria-checked={agent.mode === m.id}
              className={`${agent.mode === m.id ? "is-active" : ""} mode-${m.id} ${m.id === "reckless" && arming ? "is-arming" : ""}`}
              title={m.title}
              onClick={() => {
                if (m.id === "reckless" && agent.mode !== "reckless" && !arming) {
                  setArming(true);
                  window.setTimeout(() => setArming(false), 3000);
                  return;
                }
                setArming(false);
                agent.setMode(m.id);
              }}
            >
              {m.id === "reckless" && <FlameIcon />}
              {m.id === "reckless" && arming ? "Sure?" : m.label}
            </button>
          ))}
        </div>

        <div className="model">
          <button
            className={`model-btn ${modelOpen ? "is-open" : ""}`}
            title={`Model${agent.status?.model ? ` — last run: ${agent.status.model}` : ""}`}
            onClick={() => setModelOpen(!modelOpen)}
          >
            {MODELS.find((m) => m.id === agent.model)?.label ?? agent.model}
            <ChevronIcon open={modelOpen} />
          </button>
          {modelOpen && (
            <>
              <div className="menu-scrim" onClick={() => setModelOpen(false)} />
              <div className="menu menu-right" role="menu">
                {MODELS.map((m) => (
                  <button
                    key={m.id}
                    className={`menu-item ${m.id === agent.model ? "is-current" : ""}`}
                    role="menuitem"
                    onClick={() => {
                      agent.setModel(m.id);
                      setModelOpen(false);
                    }}
                  >
                    <span className="menu-title">{m.label}</span>
                    <span className="menu-sub">{m.title}</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>

        <IconButton title="New conversation" disabled={running || items.length === 0} onClick={agent.newChat}>
          <PlusIcon />
        </IconButton>
      </div>

      {/* ---- feed ---- */}
      <div
        className="feed"
        ref={feedRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          stick.current = atBottom;
          setScrolledUp(!atBottom);
        }}
      >
        {!project ? (
          <div className="empty-state">
            <FolderIcon size={22} />
            <p>Choose the folder Claude should work in.</p>
            <button className="btn btn-accent" onClick={pick} disabled={picking}>
              {picking ? "Choosing…" : "Choose folder"}
            </button>
          </div>
        ) : items.length === 0 ? (
          <div className="empty-state">
            <p>
              What should Claude do in <b>{basename(project)}</b>?
            </p>
            <div className="suggestions">
              {SUGGESTIONS.map((s) => (
                <button
                  key={s}
                  className="suggestion"
                  onClick={() => {
                    setDraft(s);
                    inputRef.current?.focus();
                  }}
                >
                  {s}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <Feed items={items} onAllow={(rules) => agent.send("Allowed. Please continue.", rules)} running={running} />
        )}
      </div>
      {scrolledUp && (
        <button
          className="jump-latest"
          onClick={() => {
            const el = feedRef.current;
            stick.current = true;
            el?.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
          }}
        >
          ↓ Latest
        </button>
      )}

      {agent.error && <p className="error-text">{agent.error}</p>}

      {agent.mode !== "reckless" && <ChangesBar review={review} disabled={running} />}

      {/* ---- composer ---- */}
      <form
        className={`composer ${running ? "is-running" : ""}`}
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <textarea
          ref={inputRef}
          rows={1}
          value={draft}
          disabled={!project}
          placeholder={
            !project
              ? "Choose a project folder first"
              : running
                ? "Claude is working — type your next message…"
                : lastDone
                  ? "Reply, or ask for something else…"
                  : "Ask Claude to build, fix or explain something…"
          }
          onChange={(e) => setDraft(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
        />
        {running ? (
          <button type="button" className="send-btn is-stop" title="Stop" aria-label="Stop" onClick={agent.stop}>
            <span className="stop-sq" />
          </button>
        ) : (
          <button type="submit" className="send-btn" title="Send (Enter)" aria-label="Send" disabled={!draft.trim() || !project}>
            <SendIcon />
          </button>
        )}
      </form>
    </div>
  );
}

// ---- feed rendering ---------------------------------------------------------------

type Row =
  | { kind: "item"; item: FeedItem; key: string }
  | { kind: "group"; icon: StepIcon; steps: Extract<FeedItem, { type: "step" }>[]; key: string };

/** Runs of 2+ consecutive reads/searches collapse into one "Read 4 files" row. */
function toRows(items: FeedItem[]): Row[] {
  const rows: Row[] = [];
  items.forEach((item, i) => {
    const prev = rows[rows.length - 1];
    if (item.type === "step" && (item.icon === "read" || item.icon === "search")) {
      if (prev?.kind === "group" && prev.icon === item.icon && prev.steps[0].run === item.run) {
        prev.steps.push(item);
        return;
      }
      if (prev?.kind === "item" && prev.item.type === "step" && prev.item.icon === item.icon && prev.item.run === item.run) {
        rows[rows.length - 1] = { kind: "group", icon: item.icon, steps: [prev.item, item], key: prev.key };
        return;
      }
    }
    rows.push({ kind: "item", item, key: `${item.type}-${item.run}-${item.type === "step" ? item.id : i}` });
  });
  return rows;
}

function Feed({ items, running, onAllow }: { items: FeedItem[]; running: boolean; onAllow: (rules: string[]) => void }) {
  const rows = useMemo(() => toRows(items), [items]);
  const lastRun = items[items.length - 1]?.run;
  return (
    <>
      {rows.map((row) =>
        row.kind === "group" ? (
          <GroupRow key={row.key} icon={row.icon} steps={row.steps} />
        ) : (
          <Fragment key={row.key}>
            {row.item.type === "prompt" && <div className="prompt">{row.item.text}</div>}
            {row.item.type === "text" && <RichText text={row.item.text} />}
            {row.item.type === "step" && <StepRow step={row.item} />}
            {row.item.type === "done" && (
              <DoneRow done={row.item} canAllow={!running && row.item.run === lastRun} onAllow={onAllow} />
            )}
          </Fragment>
        ),
      )}
      {running && !items.some((i) => i.type === "step" && i.state === "running") && (
        <div className="thinking">
          <span className="mini-spinner" /> Thinking…
        </div>
      )}
    </>
  );
}

function StateMark({ state }: { state: "running" | "ok" | "error" }) {
  if (state === "running") return <span className="mini-spinner" />;
  return <span className={`mark mark-${state}`}>{state === "ok" ? <CheckIcon /> : <CrossIcon />}</span>;
}

function StepRow({ step }: { step: Extract<FeedItem, { type: "step" }> }) {
  const [open, setOpen] = useState(false);
  const expandable = !!(step.detail || step.output);
  return (
    <div className={`step step-${step.icon} is-${step.state}`}>
      <button className="step-head" disabled={!expandable} onClick={() => setOpen(!open)}>
        <span className="step-icon">
          <StepGlyph icon={step.icon} />
        </span>
        <span className="step-label">{step.label}</span>
        {expandable && <ChevronIcon open={open} />}
        <StateMark state={step.state} />
      </button>
      {open && (
        <div className="step-body">
          {step.detail && <pre className="step-detail">{step.detail}</pre>}
          {step.output && <pre className="step-output">{step.output}</pre>}
        </div>
      )}
    </div>
  );
}

function GroupRow({ icon, steps }: { icon: StepIcon; steps: Extract<FeedItem, { type: "step" }>[] }) {
  const [open, setOpen] = useState(false);
  const state = steps.some((s) => s.state === "running") ? "running" : steps.some((s) => s.state === "error") ? "error" : "ok";
  const label = icon === "read" ? `Read ${steps.length} files` : `Searched ${steps.length} times`;
  return (
    <div className={`step step-${icon} is-${state}`}>
      <button className="step-head" onClick={() => setOpen(!open)}>
        <span className="step-icon">
          <StepGlyph icon={icon} />
        </span>
        <span className="step-label">{label}</span>
        <ChevronIcon open={open} />
        <StateMark state={state} />
      </button>
      {open && (
        <div className="step-children">
          {steps.map((s) => (
            <StepRow key={s.id} step={s} />
          ))}
        </div>
      )}
    </div>
  );
}

function DoneRow({ done, canAllow, onAllow }: { done: Extract<FeedItem, { type: "done" }>; canAllow: boolean; onAllow: (rules: string[]) => void }) {
  return (
    <>
      {done.denied.length > 0 && (
        <div className="denied">
          <div className="denied-head">
            <AlertIcon /> Claude wasn't allowed to:
          </div>
          <ul>
            {done.denied.map((d) => (
              <li key={d.rule} title={d.rule}>
                {d.label}
              </li>
            ))}
          </ul>
          {canAllow && (
            <button className="btn btn-accent btn-sm" onClick={() => onAllow(done.denied.map((d) => d.rule))}>
              Allow and continue
            </button>
          )}
        </div>
      )}
      <div className={`done ${done.stopped ? "is-stopped" : done.ok ? "is-ok" : "is-error"}`}>
        {done.stopped ? <span className="stop-sq" /> : done.ok ? <CheckIcon /> : <CrossIcon />}
        <span>
          {done.stopped
            ? "Stopped"
            : done.ok
              ? `Done in ${formatSpan(done.duration_ms)}${done.steps ? ` · ${done.steps} step${done.steps === 1 ? "" : "s"}` : ""}`
              : (done.message ?? "Something went wrong")}
        </span>
      </div>
    </>
  );
}

/** Claude's text: ``` fences as code blocks, `inline` code, everything else as-is. */
function RichText({ text }: { text: string }) {
  const parts = text.split(/```[^\n]*\n?/);
  return (
    <div className="answer">
      {parts.map((part, i) =>
        i % 2 === 1 ? (
          <pre key={i} className="code-block">
            {part.replace(/\n$/, "")}
          </pre>
        ) : (
          <span key={i}>
            {part.split(/(`[^`\n]+`)/).map((chunk, j) =>
              chunk.startsWith("`") && chunk.endsWith("`") && chunk.length > 2 ? <code key={j}>{chunk.slice(1, -1)}</code> : chunk,
            )}
          </span>
        ),
      )}
    </div>
  );
}
