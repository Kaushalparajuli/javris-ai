import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export interface GoogleState {
  configured: boolean;
  connected: boolean;
  email: string;
  services: { id: string; label: string; connected: boolean; email: string }[];
}

/** Connection state of one Google service ("mail", "calendar", …), and its sign-in action. */
export function useGoogle(service: string) {
  const [state, setState] = useState<GoogleState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const refresh = useCallback(() => invoke<GoogleState>("google_status").then((g) => setState({ ...g, connected: !!g.services.find((x) => x.id === service)?.connected, email: g.services.find((x) => x.id === service)?.email ?? "" })).catch(() => setState({ configured: false, connected: false, email: "", services: [] })), [service]);
  useEffect(() => {
    refresh();
  }, [refresh]);
  const connect = async () => {
    setBusy(true);
    setError("");
    try {
      await invoke("google_connect", { service });
      await refresh();
    } catch (e) {
      setError(String(e));
    }
    setBusy(false);
  };
  return { state, busy, error, connect, refresh };
}

/** Shown instead of a page's content until Google is set up and connected. */
export default function GateGoogle({
  what,
  app,
  google,
  onOpenSettings,
}: {
  what: string;
  /** The app's name, for the button. Defaults to Gmail or Google Calendar. */
  app?: string;
  google: ReturnType<typeof useGoogle>;
  onOpenSettings: () => void;
}) {
  const { state, busy, error, connect } = google;
  const appName = app ?? (what === "mail" ? "Gmail" : "Google Calendar");
  if (!state) return <div className="pad muted">Checking your Google account…</div>;
  return (
    <div className="gate">
      <h3>{state.configured ? `Connect ${appName} to see your ${what}` : "Set up Google first"}</h3>
      <p className="muted">
        {state.configured
          ? "Jarvis opens Google's sign-in page in your browser. It only asks for what this page needs, and it never sends anything without asking you."
          : "This build of Jarvis has no Google sign-in details. Add them to the .env file and rebuild."}
      </p>
      {error && <p className="error-text">{error}</p>}
      {state.configured ? (
        <button className="btn primary" onClick={connect} disabled={busy}>
          {busy ? "Waiting for Google…" : `Connect ${appName}`}
        </button>
      ) : (
        <button className="btn" onClick={onOpenSettings}>
          Open Settings
        </button>
      )}
    </div>
  );
}
