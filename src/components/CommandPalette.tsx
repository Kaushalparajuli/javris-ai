import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

/** Typed commands. Each prefix turns into a plain request that Jarvis acts on like a spoken one. */
const COMMANDS: { cmd: string; hint: string; make: (rest: string) => string }[] = [
  { cmd: "research", hint: "research <topic>", make: (r) => `Research this for me: ${r}` },
  { cmd: "quick", hint: "quick <question>", make: (r) => `Quick check, no deep dive: ${r}` },
  { cmd: "explain", hint: "explain this", make: (r) => `Explain this, from what I'm looking at.${r ? ` ${r}` : ""}` },
  { cmd: "rewrite", hint: "rewrite this <how>", make: (r) => `Rewrite my selected text${r ? `: ${r}` : " to be clearer and more professional"}. Show it to me before replacing it.` },
  { cmd: "reply", hint: "reply to this", make: (r) => `Write a reply to this, from what I'm looking at.${r ? ` ${r}` : ""} Show it to me before replacing anything.` },
  { cmd: "fix", hint: "fix this in <project>", make: (r) => `Fix this error${r ? `: ${r}` : ""}. Use what's on my screen.` },
  { cmd: "look", hint: "look <question>", make: (r) => `Look at my screen${r ? ` and tell me: ${r}` : " and tell me what you see"}.` },
  { cmd: "email", hint: "email <what>", make: (r) => `Email: ${r}` },
  { cmd: "calendar", hint: "calendar <when>", make: (r) => `What's on my calendar ${r || "today"}?` },
  { cmd: "day", hint: "day", make: () => "What needs my attention today? Catch me up." },
  { cmd: "remember", hint: "remember <fact>", make: (r) => `Remember that ${r}` },
  { cmd: "search", hint: "search <question>", make: (r) => `Search Google and answer briefly: ${r}` },
  { cmd: "drive", hint: "drive <words>", make: (r) => `Find files in my Google Drive about: ${r}` },
  { cmd: "doc", hint: "doc <what to write>", make: (r) => `Create a Google Doc: ${r}` },
  { cmd: "sheet", hint: "sheet <what>", make: (r) => `In Google Sheets: ${r}` },
  { cmd: "youtube", hint: "youtube <topic>", make: (r) => `Search YouTube for: ${r}` },
  { cmd: "browse", hint: "browse <task>", make: (r) => `In the browser: ${r}` },
];

export default function CommandPalette({ onSend, onClose }: { onSend: (text: string) => void; onClose: () => void }) {
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);

  // What Jarvis noted when the palette opened, so the user can see what "this" means.
  const [seen, setSeen] = useState<{ app: string; window: string; selection: string } | null>(null);
  useEffect(() => {
    invoke<{ app: string; window: string; selection: string }>("get_context").then(setSeen).catch(() => setSeen(null));
  }, []);
  const quick = seen?.selection
    ? [
        { label: "Explain", text: COMMANDS[2].make("") },
        { label: "Rewrite", text: COMMANDS[3].make("") },
        { label: "Reply", text: COMMANDS[4].make("") },
        { label: "Fix this error", text: COMMANDS[5].make("") },
      ]
    : [];

  const typed = text.replace(/^[>/]\s*/, "");
  const word = typed.split(/\s+/)[0].toLowerCase();
  const rest = typed.slice(word.length).trim();
  const exact = COMMANDS.find((c) => c.cmd === word);
  const matches = exact ? [exact] : COMMANDS.filter((c) => word && c.cmd.startsWith(word));

  const run = () => {
    if (!text.trim()) return;
    // A known command becomes its request; anything else goes through as typed.
    onSend(exact ? exact.make(rest) : text.trim());
    onClose();
  };

  return (
    <div className="overlay palette-overlay" onClick={onClose}>
      <div className="palette glass" role="dialog" aria-label="Type a command" onClick={(e) => e.stopPropagation()}>
        <input
          ref={input}
          value={text}
          placeholder="Type a command or ask anything…"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            else if (e.key === "Enter") run();
            else if (e.key === "Tab" && matches.length && !exact) {
              e.preventDefault();
              setText(`${matches[0].cmd} `);
            }
          }}
        />
        {seen?.app && (
          <p className="palette-seen">
            Looking at <b>{seen.app}</b>
            {seen.window ? ` · ${seen.window.length > 40 ? `${seen.window.slice(0, 40)}…` : seen.window}` : ""}
            {seen.selection ? ` · ${seen.selection.length} characters selected` : " · nothing selected"}
          </p>
        )}
        {quick.length > 0 && !text && (
          <div className="palette-quick">
            {quick.map((q) => (
              <button
                key={q.label}
                type="button"
                className="mini"
                onClick={() => {
                  onSend(q.text);
                  onClose();
                }}
              >
                {q.label}
              </button>
            ))}
          </div>
        )}
        <ul>
          {(matches.length ? matches : COMMANDS).slice(0, 7).map((c) => (
            <li key={c.cmd}>
              <button type="button" onClick={() => { setText(`${c.cmd} `); input.current?.focus(); }}>
                <b>{c.cmd}</b>
                <span>{c.hint}</span>
              </button>
            </li>
          ))}
        </ul>
        <p className="hint">Enter to send · Tab to complete · Esc to close. "this" means the app you were just in.</p>
      </div>
    </div>
  );
}
