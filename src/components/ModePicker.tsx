import { useEffect, useRef, useState } from "react";
import { MODES, setMode, useMode, type Mode } from "../lib/mode";

const ICON: Record<Mode, string> = {
  show: "M12 5C7 5 3 9 2 12c1 3 5 7 10 7s9-4 10-7c-1-3-5-7-10-7zm0 11a4 4 0 1 1 0-8 4 4 0 0 1 0 8zm0-6a2 2 0 1 0 0 4 2 2 0 0 0 0-4z",
  auto: "M13 2 4 14h6l-1 8 9-12h-6z",
  manual: "M9 11V5a2 2 0 1 1 4 0v5h1V8a2 2 0 1 1 4 0v6c0 4-3 7-7 7h-1c-3 0-4-2-6-5l-1-2a2 2 0 0 1 3-2l2 2V11z",
};

/** The mode under the prompt box: Show (plan first), Auto, or Manual. Shift+Tab in the box cycles them. */
export default function ModePicker() {
  const mode = useMode();
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const current = MODES.find((m) => m.id === mode)!;

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => !box.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <div className="mode-picker" ref={box}>
      <button type="button" className={`mode-btn m-${mode}`} onClick={() => setOpen(!open)} aria-haspopup="menu" aria-expanded={open} title={`${current.label} mode: ${current.hint} (⇧Tab to switch)`}>
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d={ICON[mode]} />
        </svg>
        {current.label}
        <i aria-hidden="true">▴</i>
      </button>
      {open && (
        <div className="mode-menu" role="menu">
          {MODES.map((m) => (
            <button
              key={m.id}
              type="button"
              role="menuitemradio"
              aria-checked={m.id === mode}
              className={m.id === mode ? "on" : ""}
              onClick={() => {
                setMode(m.id);
                setOpen(false);
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d={ICON[m.id]} />
              </svg>
              <span>
                <b>{m.label}</b>
                <small>{m.hint}</small>
              </span>
              {m.id === mode && <em aria-hidden="true">✓</em>}
            </button>
          ))}
          <div className="mode-foot">⇧Tab switches mode</div>
        </div>
      )}
    </div>
  );
}
