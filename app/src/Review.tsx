import { useEffect, useRef, useState } from "react";
import { api, FileChange, on, ReviewStatus } from "./api";
import { CheckIcon, ChevronIcon, StepGlyph } from "./ui";

export function useReview() {
  const [review, setReview] = useState<ReviewStatus>({ files: [], live: false, vscode: false });
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
  const added = review.files.reduce((n, f) => n + f.added, 0);
  const removed = review.files.reduce((n, f) => n + f.removed, 0);
  return {
    ...review,
    added,
    removed,
    error,
    accept: (path?: string) => call(api.reviewAccept(path)),
    reject: (path?: string) => call(api.reviewReject(path)),
    diff: (path: string) => call(api.reviewDiff(path)),
    setLive: (live: boolean) => call(api.reviewSetLive(live)),
  };
}

export type ReviewState = ReturnType<typeof useReview>;

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

function Counts({ added, removed }: { added: number; removed: number }) {
  return (
    <span className="counts">
      <span className="plus">+{added}</span> <span className="minus">−{removed}</span>
    </span>
  );
}

const STATUS_LETTER: Record<FileChange["status"], string> = { modified: "M", created: "A", deleted: "D" };

/** "3 files changed" strip in the Code tab, with the file list, live diff and accept / reject. */
export function ChangesBar({ review, disabled }: { review: ReviewState; disabled: boolean }) {
  const [open, setOpen] = useState(false);
  const n = review.files.length;
  if (n === 0) return null;
  return (
    <div className={`changes ${open ? "is-open" : ""}`}>
      <div className="changes-head">
        <button className="changes-toggle" onClick={() => setOpen(!open)}>
          <StepGlyph icon="edit" />
          <span>
            {n} file{n === 1 ? "" : "s"} changed
          </span>
          <Counts added={review.added} removed={review.removed} />
          <ChevronIcon open={open} />
        </button>
        <button
          className={`live-toggle ${review.live ? "is-on" : ""}`}
          disabled={!review.vscode}
          title={review.vscode ? "Live diff: open each changed file in VS Code and follow it as Claude edits" : "VS Code not found"}
          onClick={() => review.setLive(!review.live)}
        >
          <span className="live-dot" /> Live diff
        </button>
        <ConfirmButton
          className="btn btn-ghost btn-sm reject"
          label="Reject all"
          confirm="Undo all?"
          title="Put every file back the way it was"
          onConfirm={() => !disabled && review.reject()}
        />
        <button className="btn btn-accent btn-sm" onClick={() => review.accept()}>
          Accept all
        </button>
      </div>
      {open && (
        <div className="changes-list">
          {review.files.map((f) => (
            <div key={f.path} className="change">
              <span className={`change-status is-${f.status}`}>{STATUS_LETTER[f.status]}</span>
              <span className="change-name" title={f.path}>
                {f.name}
              </span>
              <Counts added={f.added} removed={f.removed} />
              {review.vscode && (
                <button className="mini-btn" title="Show the diff in VS Code" onClick={() => review.diff(f.path)}>
                  Diff
                </button>
              )}
              <ConfirmButton
                className="mini-btn reject"
                label="Reject"
                confirm="Undo?"
                title="Put this file back the way it was"
                onConfirm={() => !disabled && review.reject(f.path)}
              />
              <button className="mini-btn accept" title="Keep this change" onClick={() => review.accept(f.path)}>
                <CheckIcon />
              </button>
            </div>
          ))}
        </div>
      )}
      {review.error && <p className="error-text">{review.error}</p>}
    </div>
  );
}

/** Drops below the collapsed notch while changes wait for a decision. */
export function ReviewPill({
  review,
  onOpen,
  onHover,
}: {
  review: ReviewState;
  onOpen: () => void;
  onHover: (over: boolean) => void;
}) {
  const n = review.files.length;
  return (
    <div
      className="review-pill"
      onPointerEnter={() => onHover(true)}
      onPointerLeave={() => onHover(false)}
      onClick={(e) => e.stopPropagation()}
    >
      <button className="pill-summary" title="Review the changes" onClick={onOpen}>
        <StepGlyph icon="edit" />
        <span>
          {n} file{n === 1 ? "" : "s"} changed
        </span>
        <Counts added={review.added} removed={review.removed} />
      </button>
      <ConfirmButton className="pill-btn reject" label="Reject" confirm="Undo all?" title="Put every file back the way it was" onConfirm={() => review.reject()} />
      <button className="pill-btn accept" title="Keep all changes" onClick={() => review.accept()}>
        <CheckIcon /> Accept
      </button>
    </div>
  );
}
