import { useEffect, useRef, useState } from "react";
import { api, FileChange, on, ReviewStatus } from "./api";
import { basename, CheckIcon, StepGlyph } from "./ui";

export function useReview() {
  const [review, setReview] = useState<ReviewStatus>({ projects: {}, live: false, vscode: false });
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.reviewStatus().then(setReview).catch(() => {});
    const sub = on<ReviewStatus>("review", setReview);
    return () => {
      sub.then((un) => un());
    };
  }, []);
  const call = (p: Promise<unknown>) => {
    setError(null);
    p.catch((e) => setError(String(e)));
  };
  const forProject = (project: string): ProjectReview => {
    const files = review.projects[project] ?? [];
    return {
      project,
      files,
      added: files.reduce((n, f) => n + f.added, 0),
      removed: files.reduce((n, f) => n + f.removed, 0),
      vscode: review.vscode,
      accept: (path?: string) => call(api.reviewAccept(project, path)),
      reject: (path?: string) => call(api.reviewReject(project, path)),
      diffAll: () => files.forEach((f) => call(api.reviewDiff(project, f.path))),
    };
  };
  /** projects with changes waiting for a decision */
  const pending = Object.keys(review.projects).filter((p) => review.projects[p].length > 0);
  return { ...review, error, forProject, pending };
}

export type ReviewState = ReturnType<typeof useReview>;

export interface ProjectReview {
  project: string;
  files: FileChange[];
  added: number;
  removed: number;
  vscode: boolean;
  accept: (path?: string) => void;
  reject: (path?: string) => void;
  diffAll: () => void;
}

/** A destructive button that needs a second click within 3 s. */
function ConfirmButton({ className, label, confirm, title, onConfirm }: { className: string; label: string; confirm: string; title: string; onConfirm: () => void }) {
  const [armed, setArmed] = useState(false);
  const timer = useRef<number | null>(null);
  useEffect(() => () => void (timer.current && window.clearTimeout(timer.current)), []);
  return (
    <button
      className={`${className} ${armed ? "is-armed" : ""}`}
      title={title}
      onClick={(e) => {
        e.stopPropagation();
        if (armed) {
          setArmed(false);
          onConfirm();
          return;
        }
        setArmed(true);
        timer.current = window.setTimeout(() => setArmed(false), 3000);
      }}
    >
      {armed ? confirm : label}
    </button>
  );
}

export function Counts({ added, removed }: { added: number; removed: number }) {
  return (
    <span className="counts">
      <span className="plus">+{added}</span> <span className="minus">−{removed}</span>
    </span>
  );
}

/** pixels each notification takes under the notch */
export const PILL_STEP = 42;
/** widest a notification gets when it opens sideways */
export const PILL_WIDTH = 460;

/**
 * Changes waiting for a decision, as notifications stacked under the collapsed
 * notch (one per project). Hovering one slides it open sideways: Reject on the
 * left, Diff and Accept on the right.
 */
export function ReviewPills({ review, busy, onHover }: { review: ReviewState; busy: (project: string) => boolean; onHover: (over: boolean) => void }) {
  const many = review.pending.length > 1;
  return (
    <div className="pills" onPointerEnter={() => onHover(true)} onPointerLeave={() => onHover(false)} onClick={(e) => e.stopPropagation()}>
      {review.pending.slice(0, 3).map((p, i) => (
        <ReviewPill key={p} r={review.forProject(p)} showProject={many} busy={busy(p)} index={i} />
      ))}
    </div>
  );
}

function ReviewPill({ r, showProject, busy, index }: { r: ProjectReview; showProject: boolean; busy: boolean; index: number }) {
  const n = r.files.length;
  return (
    <div className="pill" style={{ top: index * PILL_STEP, animationDelay: `${index * 60}ms` }} title={r.files.map((f) => f.name).join("\n")}>
      <span className="pill-side pill-left">
        <ConfirmButton
          className="pill-btn reject"
          label="Reject"
          confirm="Undo all?"
          title="Put every file back the way it was"
          onConfirm={() => !busy && r.reject()}
        />
      </span>
      <span className="pill-summary">
        <StepGlyph icon="edit" />
        {showProject && <b className="pill-project">{basename(r.project)}</b>}
        <span>
          {n} file{n === 1 ? "" : "s"}
        </span>
        <Counts added={r.added} removed={r.removed} />
      </span>
      <span className="pill-side pill-right">
        {r.vscode && (
          <button className="pill-btn diff" title="Open every change side by side in VS Code" onClick={r.diffAll}>
            Diff
          </button>
        )}
        <button className="pill-btn accept" title="Keep all changes" onClick={() => r.accept()}>
          <CheckIcon /> Accept
        </button>
      </span>
    </div>
  );
}

