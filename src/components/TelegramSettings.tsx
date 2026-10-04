import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { Settings } from "../lib/types";

interface TelegramStatus {
  enabled: boolean;
  paired: boolean;
  configured: boolean;
  running: boolean;
  pairing: boolean;
  botName: string;
  botUsername: string;
  error: string;
}

interface PairStart {
  code: string;
  botName: string;
  botUsername: string;
}

type TelegramFields = Pick<Settings, "telegramToken" | "telegramChatId" | "telegramEnabled">;

/** Save only the Telegram fields, on top of the settings as they are now, then apply them. */
async function saveTelegram(change: Partial<TelegramFields>) {
  const fresh = await invoke<Settings>("get_settings");
  await invoke("save_settings", { settings: { ...fresh, ...change } });
  await invoke("telegram_restart");
}

/**
 * Telegram remote, a self-contained Settings section: the owner's own bot, paired to their chat,
 * so they can use Jarvis from their phone. Saves straight away; it doesn't wait for Settings' Save.
 */
export default function TelegramSettings() {
  const [token, setToken] = useState("");
  const [saved, setSaved] = useState<TelegramFields | null>(null);
  const [status, setStatus] = useState<TelegramStatus | null>(null);
  const [pair, setPair] = useState<PairStart | null>(null);
  const [busy, setBusy] = useState("");
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const refresh = async () => {
    const s = await invoke<Settings>("get_settings").catch(() => null);
    if (s) {
      setSaved({ telegramToken: s.telegramToken ?? "", telegramChatId: s.telegramChatId ?? 0, telegramEnabled: !!s.telegramEnabled });
      setToken((t) => t || s.telegramToken || "");
    }
    const st = await invoke<TelegramStatus>("telegram_status").catch(() => null);
    setStatus(st);
    if (st && !st.pairing) setPair(null);
  };

  useEffect(() => {
    refresh();
    const un = listen("telegram-paired", () => {
      setPair(null);
      setMsg({ ok: true, text: "Paired. Send your bot a message to try it." });
      refresh();
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // While a code is waiting, check now and then in case it expires.
  useEffect(() => {
    if (!pair) return;
    const t = setInterval(refresh, 4000);
    return () => clearInterval(t);
  }, [pair]);

  const run = async (what: string, fn: () => Promise<void>) => {
    setBusy(what);
    setMsg(null);
    try {
      await fn();
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    }
    setBusy("");
    refresh();
  };

  const trimmed = token.trim();
  const tokenChanged = !!saved && trimmed !== saved.telegramToken;
  const looksLikeToken = /^\d+:[\w-]{20,}$/.test(trimmed);

  const saveToken = () =>
    run("token", async () => {
      // A different bot means pairing again.
      await saveTelegram({ telegramToken: trimmed, telegramChatId: 0, telegramEnabled: false });
      setMsg({ ok: true, text: trimmed ? "Token saved. Now press Pair." : "Token removed." });
    });

  const startPair = () =>
    run("pair", async () => {
      if (tokenChanged) await saveTelegram({ telegramToken: trimmed, telegramChatId: 0, telegramEnabled: false });
      setPair(await invoke<PairStart>("telegram_pair_start"));
    });

  const cancelPair = () => run("pair", async () => invoke("telegram_pair_cancel"));

  const setEnabled = (on: boolean) => run("enable", () => saveTelegram({ telegramEnabled: on }));

  const unpair = () => run("unpair", () => saveTelegram({ telegramChatId: 0, telegramEnabled: false }));

  const paired = !!saved?.telegramChatId;
  const bot = status?.botUsername ? `@${status.botUsername}` : "your bot";
  const deepLink = pair?.botUsername ? `https://t.me/${pair.botUsername}?start=${pair.code}` : "";

  return (
    <>
      <div className="label">Telegram</div>
      <p className="hint">
        Use Jarvis from your phone: message your own Telegram bot and Jarvis answers in writing, asks you to approve things with buttons, and sends you the reports, documents
        and decks it makes. Only the chat you pair can talk to it.
      </p>

      <label htmlFor="tg-token">1. Make a bot with @BotFather in Telegram (send it /newbot), then paste the token it gives you</label>
      <div className="row" style={{ gap: 8, alignItems: "center" }}>
        <input
          id="tg-token"
          type="password"
          autoComplete="off"
          value={token}
          placeholder="123456789:AA…"
          onChange={(e) => setToken(e.target.value)}
        />
        <button className="mini" type="button" onClick={saveToken} disabled={!tokenChanged || (!!trimmed && !looksLikeToken) || !!busy}>
          {busy === "token" ? "Saving…" : "Save"}
        </button>
        <button className="mini" type="button" onClick={() => openUrl("https://t.me/BotFather").catch(() => {})}>
          Open @BotFather
        </button>
      </div>
      {!!trimmed && !looksLikeToken && <p className="hint warn">That doesn't look like a bot token. It's two parts with a colon, like 123456789:AAE…</p>}

      <label>2. Pair your phone</label>
      {pair ? (
        <div className="loginbox">
          <p className="hint">
            Send this to {pair.botUsername ? `@${pair.botUsername}` : "your bot"} within ten minutes: <b>/start {pair.code}</b>
          </p>
          <div className="row" style={{ gap: 8 }}>
            {deepLink && (
              <button className="mini" type="button" onClick={() => openUrl(deepLink).catch(() => {})}>
                Open in Telegram
              </button>
            )}
            <button className="mini" type="button" onClick={cancelPair} disabled={!!busy}>
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div className="row" style={{ gap: 8 }}>
          <button className="mini" type="button" onClick={startPair} disabled={!trimmed || !looksLikeToken || !!busy}>
            {busy === "pair" ? "Checking the token…" : paired ? "Pair again" : "Pair"}
          </button>
        </div>
      )}

      <label>3. Status</label>
      <div className={`status-line ${paired && saved?.telegramEnabled && status?.running ? "ok" : "bad"}`}>
        <i />
        <span>
          {!status
            ? "Checking…"
            : !status.configured
              ? "No bot yet."
              : !paired
                ? status.pairing
                  ? `Waiting for the code from your phone (${bot}).`
                  : `${bot} isn't paired yet.`
                : !saved?.telegramEnabled
                  ? `Paired with ${bot}, switched off.`
                  : status.running
                    ? `On. Message ${bot} to talk to Jarvis.`
                    : `Paired with ${bot}, but not listening.`}
        </span>
        {paired && (
          <button className="mini" type="button" onClick={unpair} disabled={!!busy}>
            Unpair
          </button>
        )}
      </div>
      {paired && (
        <label className="check">
          <input type="checkbox" checked={!!saved?.telegramEnabled} onChange={(e) => setEnabled(e.target.checked)} disabled={!!busy} />
          Listen for messages from my phone
        </label>
      )}
      {status?.error && <p className="hint warn">{status.error}</p>}
      {msg?.text && <p className={`hint ${msg.ok ? "" : "warn"}`}>{msg.text}</p>}
      <p className="hint">
        Messages go through Telegram's servers. Jarvis only answers while it's running on this Mac; anything that needs your OK is asked on your phone and on screen, and
        the first answer counts.
      </p>
    </>
  );
}
