import { useEffect, useState } from "react";
import { api, ClaudeAuthState, ClaudeStatus, GeminiStatus, on, Provider, PROVIDER_NAME, Settings as SettingsData } from "./api";
import type { Agents } from "./CodeTab";
import { MASCOTS, MascotPicker, useMascot } from "./Mascot";
import type { Voice } from "./Voice";
import { capitalize, CheckIcon, RetryIcon } from "./ui";

/** Gemini's sign-in (Antigravity CLI), kept current from the backend. */
export function useGemini() {
  const [gemini, setGemini] = useState<GeminiStatus | null>(null);
  useEffect(() => {
    api.geminiStatus().then(setGemini).catch(() => {});
    const sub = on<GeminiStatus>("gemini", setGemini);
    return () => {
      sub.then((un) => un());
    };
  }, []);
  return gemini;
}

/**
 * Settings, built to be checked at a glance and changed in one click:
 *   accounts   Claude and Gemini side by side: signed in or not, one button for
 *              the next step, and which one new sessions use
 *   general    start with Windows, voice prompts, the mascot (or none)
 */
export function Settings({ claude, gemini, agents, voice }: { claude: ClaudeStatus | null; gemini: GeminiStatus | null; agents: Agents; voice: Voice }) {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [mascot, setMascot] = useMascot();
  const skin = MASCOTS.find((m) => m.id === mascot);
  useEffect(() => {
    api.settingsGet().then(setSettings).catch(() => {});
    // the cards should tell the truth when they're opened
    api.claudeRefresh().catch(() => {});
    api.geminiRefresh().catch(() => {});
  }, []);

  const count = (p: Provider) => agents.sessions.filter((s) => s.provider === p);
  const working = (p: Provider) => count(p).filter((s) => s.running).length;

  return (
    <div className="settings">
      <section className="card settings-accounts">
        <header className="settings-head">
          <b>Accounts</b>
          <span className="muted">Both can work at the same time, each in its own sessions</span>
        </header>
        <div className="accounts">
          <AccountCard
            provider="claude"
            state={claude?.state ?? "checking"}
            message={claude?.message ?? null}
            detail={claude?.plan ? `${capitalize(claude.plan)} plan` : null}
            sessions={count("claude").length}
            working={working("claude")}
            isDefault={agents.defaultProvider === "claude"}
            onDefault={() => agents.setDefaultProvider("claude")}
            onSignIn={() => api.claudeLogin()}
            onSignOut={() => api.claudeLogout()}
            onInstall={() => api.claudeInstall()}
            onCheck={() => api.claudeRecheck()}
          />
          <AccountCard
            provider="gemini"
            state={gemini?.state ?? "checking"}
            message={gemini?.message ?? null}
            detail={gemini?.account ?? null}
            sessions={count("gemini").length}
            working={working("gemini")}
            isDefault={agents.defaultProvider === "gemini"}
            onDefault={() => agents.setDefaultProvider("gemini")}
            onSignIn={() => api.geminiLogin()}
            onSignOut={() => api.geminiLogout()}
            onInstall={() => api.geminiInstall()}
            onCheck={() => api.geminiRefresh()}
          />
        </div>
        {agents.error && <p className="error-text">{agents.error}</p>}
      </section>

      <section className="card settings-general">
        <Toggle
          label="Start with Windows"
          sub="Notch Code is there when you sign in"
          on={settings?.autostart ?? false}
          disabled={!settings}
          onChange={(on) => {
            setSettings((s) => (s ? { ...s, autostart: on } : s));
            api.setAutostart(on).catch(() => {});
          }}
        />
        <Toggle
          label="Voice prompts"
          sub="Say “Claude, …” to send a message; off keeps the microphone closed"
          on={voice.wake}
          onChange={(on) => voice.setWake(on)}
        />
        <div className="setting-row mascot-row">
          <span className="setting-text">
            <b>Mascot</b>
            <span className="muted">{skin && mascot !== "none" ? `${skin.name}, ${skin.what}` : "None"}</span>
          </span>
          <MascotPicker value={mascot} onChange={setMascot} />
        </div>
      </section>
    </div>
  );
}

const STATE_TEXT: Record<ClaudeAuthState, string> = {
  checking: "Checking…",
  installing: "Installing…",
  not_installed: "Not installed",
  signed_out: "Not signed in",
  signing_in: "Finish in your browser…",
  signed_in: "Signed in",
  error: "Needs attention",
};

function AccountCard(props: {
  provider: Provider;
  state: ClaudeAuthState;
  message: string | null;
  /** plan or account, once signed in */
  detail: string | null;
  sessions: number;
  working: number;
  isDefault: boolean;
  onDefault: () => void;
  onSignIn: () => void;
  onSignOut: () => void;
  onInstall: () => void;
  onCheck: () => void;
}) {
  const { provider, state } = props;
  const name = PROVIDER_NAME[provider];
  const [confirmOut, setConfirmOut] = useState(false);
  useEffect(() => setConfirmOut(false), [state]);
  const busy = state === "checking" || state === "installing" || state === "signing_in";
  const tone = state === "signed_in" ? "ok" : busy ? "busy" : state === "error" ? "error" : "warn";
  const usage =
    props.sessions === 0
      ? "No open sessions"
      : `${props.sessions} session${props.sessions > 1 ? "s" : ""}${props.working ? ` · ${props.working} working` : ""}`;
  // what's said under the status: the account once signed in, else what to do
  const line =
    state === "signed_in"
      ? (props.detail ?? (provider === "gemini" ? "Google account" : "Claude subscription"))
      : state === "signing_in"
        ? provider === "gemini"
          ? "A sign-in window opened — finish signing in with Google in the browser"
          : "A sign-in window opened — finish there and in the browser"
        : (props.message ?? (provider === "gemini" ? "Uses your Google account (Gemini subscription)" : "Uses your Claude subscription"));

  return (
    <div className={`account is-${provider} tone-${tone} ${props.isDefault ? "is-default" : ""}`}>
      <div className="account-top">
        <span className={`agent-logo is-${provider}`}>{provider === "gemini" ? "G" : "C"}</span>
        <div className="account-name">
          <b>{name}</b>
          <span className={`account-state tone-${tone}`}>
            {busy ? <span className="mini-spinner" /> : <span className="state-dot" />}
            {STATE_TEXT[state]}
          </span>
        </div>
        <button className="icon-btn account-check" title="Check again" aria-label="Check again" disabled={busy} onClick={props.onCheck}>
          <RetryIcon />
        </button>
      </div>

      <p className="account-line" title={line}>
        {line}
      </p>

      <div className="account-actions">
        {state === "not_installed" && (
          <button className="btn primary sm" onClick={props.onInstall}>
            Install {provider === "gemini" ? "Antigravity CLI" : "Claude Code"}
          </button>
        )}
        {(state === "signed_out" || state === "error") && (
          <button className="btn primary sm" onClick={props.onSignIn}>
            {provider === "gemini" ? "Sign in with Google" : "Sign in"}
          </button>
        )}
        {state === "signing_in" && (
          <button className="btn secondary sm" onClick={props.onSignIn} title="Open the sign-in window again">
            Open again
          </button>
        )}
        {state === "signed_in" &&
          (confirmOut ? (
            <>
              <button className="btn danger sm" onClick={props.onSignOut}>
                Sign out of {name}
              </button>
              <button className="btn secondary sm" onClick={() => setConfirmOut(false)}>
                Cancel
              </button>
            </>
          ) : (
            <button className="btn secondary sm" onClick={() => setConfirmOut(true)}>
              Sign out
            </button>
          ))}
        <span className="account-usage">{usage}</span>
      </div>

      <button
        className={`account-default ${props.isDefault ? "on" : ""}`}
        role="radio"
        aria-checked={props.isDefault}
        title={props.isDefault ? `New sessions use ${name}` : `Use ${name} for new sessions`}
        onClick={props.onDefault}
      >
        <span className="radio">{props.isDefault && <CheckIcon />}</span>
        {props.isDefault ? "New sessions use this" : "Use for new sessions"}
      </button>
    </div>
  );
}

function Toggle({ label, sub, on, disabled, onChange }: { label: string; sub: string; on: boolean; disabled?: boolean; onChange: (on: boolean) => void }) {
  return (
    <button className="setting-row" role="switch" aria-checked={on} disabled={disabled} onClick={() => onChange(!on)}>
      <span className="setting-text">
        <b>{label}</b>
        <span className="muted">{sub}</span>
      </span>
      <span className={`switch ${on ? "on" : ""}`}>
        <i />
      </span>
    </button>
  );
}
