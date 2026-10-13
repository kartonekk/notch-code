import { useEffect, useState } from "react";

// Claude Code's `/usage`, shown as a card instead of its plain text:
//
//   You are currently using your subscription to power your Claude Code usage
//
//   Current session: 58% used · resets Oct 13, 5:30pm (Europe/Warsaw)
//   Current week (all models): 44% used · resets Oct 19, 8am (Europe/Warsaw)
//
//   What's contributing to your limits usage?
//   Approximate, based on local sessions on this machine — …
//
//   Last 24h · 632 requests · 10 sessions
//     75% of your usage was at >150k context
//     Top MCP servers: smartwood-console 5%

export interface UsageMeter {
  label: string;
  /** 0..1 */
  used: number;
  /** "Oct 13, 5:30pm (Europe/Warsaw)", as Claude wrote it */
  resets: string | null;
  /** unix ms, when `resets` could be read in this machine's time zone */
  resetsAt: number | null;
  /** how long the window is (5 hours, a week), when known */
  windowMs: number | null;
}

export interface UsagePeriod {
  /** "Last 24h" */
  title: string;
  /** "632 requests", "10 sessions" */
  stats: string[];
  lines: string[];
}

export interface UsageReport {
  /** "your subscription" */
  using: string | null;
  meters: UsageMeter[];
  /** "Approximate, based on local sessions …" */
  notes: string[];
  periods: UsagePeriod[];
}

const SEP = /\s*[·•�]\s*/;
const METER = /^(.+?):\s*(\d+(?:\.\d+)?)%\s*used\b(.*)$/i;
const HOUR = 3_600_000;

/** The report, or null when `text` isn't one (any other answer). */
export function parseUsageReport(text: string, now = Date.now()): UsageReport | null {
  if (text.includes("```")) return null;
  const report: UsageReport = { using: null, meters: [], notes: [], periods: [] };
  let period: UsagePeriod | null = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const indented = /^\s/.test(raw);
    const meter = METER.exec(line);
    if (meter && !period) {
      const [, label, pct, rest] = meter;
      const resets = /resets\s+(.+)$/i.exec(rest)?.[1].trim() ?? null;
      const windowMs = /session/i.test(label) ? 5 * HOUR : /week/i.test(label) ? 7 * 24 * HOUR : null;
      report.meters.push({ label, used: Number(pct) / 100, resets, resetsAt: resets ? parseReset(resets, now) : null, windowMs });
    } else if (/^you are currently using\s/i.test(line)) {
      report.using = line.replace(/^you are currently using\s+/i, "").replace(/\s+to power\b.*$/i, "");
    } else if (/^last\s+\S+/i.test(line) && !indented) {
      const [title, ...stats] = line.split(SEP);
      period = { title, stats, lines: [] };
      report.periods.push(period);
    } else if (period && indented) {
      period.lines.push(line);
    } else if (/^what's contributing/i.test(line)) {
      continue;
    } else {
      report.notes.push(line);
    }
  }
  // only Claude Code's own report, never an answer that mentions a percentage
  if (!report.meters.some((m) => /^current\b/i.test(m.label))) return null;
  return report;
}

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/** "Oct 13, 5:30pm (Europe/Warsaw)" / "8am" → unix ms; null when it's another time zone or unreadable */
export function parseReset(text: string, now = Date.now()): number | null {
  const m = /^(?:([a-z]{3})[a-z]*\.?\s+(\d{1,2}),?\s+(?:at\s+)?)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)\b(?:\s*\(([^)]+)\))?/i.exec(text.trim());
  if (!m) return null;
  const [, month, day, hour, minute, half, zone] = m;
  if (zone && zone !== Intl.DateTimeFormat().resolvedOptions().timeZone) return null;
  const d = new Date(now);
  if (month) {
    const i = MONTHS.indexOf(month.toLowerCase());
    if (i < 0) return null;
    d.setMonth(i, Number(day));
  }
  d.setHours((Number(hour) % 12) + (half.toLowerCase() === "pm" ? 12 : 0), Number(minute ?? 0), 0, 0);
  // "Jan 2" read in late December, or a bare time that already passed today
  if (d.getTime() < now - HOUR) {
    if (month) d.setFullYear(d.getFullYear() + 1);
    else d.setDate(d.getDate() + 1);
  }
  return d.getTime();
}

/** "2 h 14 min", "3 d 4 h" */
function formatLeft(ms: number) {
  const min = Math.max(1, Math.round(ms / 60_000));
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} h${min % 60 ? ` ${min % 60} min` : ""}`;
  return `${Math.floor(h / 24)} d${h % 24 ? ` ${h % 24} h` : ""}`;
}

const tone = (used: number) => (used >= 1 ? "is-error" : used >= 0.8 ? "is-warn" : "");

/** "Current week (all models)" → "Week", "(all models)" */
function splitLabel(label: string): [string, string | null] {
  const m = /^(?:current\s+)?([^(]+?)\s*(\([^)]*\))?$/i.exec(label);
  if (!m) return [label, null];
  return [m[1].charAt(0).toUpperCase() + m[1].slice(1), m[2]?.slice(1, -1) ?? null];
}

export function UsageCard({ report }: { report: UsageReport }) {
  // the "resets in …" countdown
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(t);
  }, []);
  return (
    <div className="usage-card">
      <div className="usage-head">
        <span className="usage-title">Plan usage</span>
        {report.using && <span className="usage-using">{report.using}</span>}
      </div>
      <div className="usage-meters">
        {report.meters.map((m) => (
          <Meter key={m.label} meter={m} now={now} />
        ))}
      </div>
      {report.periods.length > 0 && (
        <div className="usage-periods">
          {report.periods.map((p) => (
            <Period key={p.title} period={p} />
          ))}
        </div>
      )}
      {report.notes.length > 0 && <div className="usage-note">{report.notes.join(" ")}</div>}
    </div>
  );
}

function Meter({ meter, now }: { meter: UsageMeter; now: number }) {
  const [name, scope] = splitLabel(meter.label);
  const pct = Math.round(meter.used * 100);
  const left = meter.resetsAt != null ? meter.resetsAt - now : null;
  // how far through its window the clock is: usage ahead of this mark runs out early
  const pace =
    left != null && meter.windowMs != null && left > 0 && left <= meter.windowMs ? 1 - left / meter.windowMs : null;
  const ahead = pace != null && meter.used > pace + 0.05;
  return (
    <div className={`usage-meter ${tone(meter.used)}`}>
      <div className="usage-meter-head">
        <span className="usage-name">
          {name}
          {scope && <span className="usage-scope">{scope}</span>}
        </span>
        <span className="usage-pct">
          {pct}
          <small>%</small>
        </span>
      </div>
      <div className="usage-bar">
        <i style={{ width: `${Math.min(100, pct)}%` }} />
        {pace != null && (
          <b
            style={{ left: `${pace * 100}%` }}
            title={`${Math.round(pace * 100)}% of this window has passed${ahead ? ": using it faster than it refills" : ""}`}
          />
        )}
      </div>
      {meter.resets && (
        <div className="usage-reset" title={`resets ${meter.resets}`}>
          {left != null && left > 0 ? (
            <>
              resets in <span className="usage-left">{formatLeft(left)}</span> ·{" "}
              {new Date(meter.resetsAt!).toLocaleString([], {
                weekday: left > 20 * HOUR ? "short" : undefined,
                hour: "2-digit",
                minute: "2-digit",
              })}
            </>
          ) : left != null ? (
            <>reset since</>
          ) : (
            <>resets {meter.resets}</>
          )}
          {ahead && <span className="usage-ahead">ahead of pace</span>}
        </div>
      )}
    </div>
  );
}

function Period({ period }: { period: UsagePeriod }) {
  return (
    <div className="usage-period">
      <div className="usage-period-head">
        <span className="usage-period-title">{period.title}</span>
        {period.stats.map((s) => (
          <span key={s} className="usage-stat">
            {s}
          </span>
        ))}
      </div>
      {period.lines.map((l) => (
        <PeriodLine key={l} line={l} />
      ))}
    </div>
  );
}

/** "75% of your usage was at >150k context" gets a bar; "Top MCP servers: a 5%, b 2%" gets chips */
function PeriodLine({ line }: { line: string }) {
  const share = /^(\d+(?:\.\d+)?)%\s+(.+)$/.exec(line);
  if (share) {
    const pct = Number(share[1]);
    return (
      <div className="usage-line">
        <span className="usage-mini">
          <i style={{ width: `${Math.min(100, pct)}%` }} />
        </span>
        <span className="usage-line-pct">{share[1]}%</span>
        <span className="usage-line-text">{share[2].replace(/^of your usage\s+(?:was\s+)?/i, "")}</span>
      </div>
    );
  }
  const listed = /^([^:]{1,40}):\s*(.+)$/.exec(line);
  if (listed) {
    return (
      <div className="usage-line">
        <span className="usage-line-text">{listed[1]}</span>
        <span className="usage-chips">
          {listed[2].split(/\s*,\s*/).map((item) => {
            const m = /^(.*?)\s+(\d+(?:\.\d+)?%)$/.exec(item);
            return (
              <span key={item} className="usage-chip">
                {m ? m[1] : item}
                {m && <span className="usage-chip-pct">{m[2]}</span>}
              </span>
            );
          })}
        </span>
      </div>
    );
  }
  return (
    <div className="usage-line">
      <span className="usage-line-text">{line}</span>
    </div>
  );
}
