import { useEffect, useRef, useState } from "react";

/** Pages reached from the sidebar's account menu rather than its main list. */
export type MenuPage = "mail" | "calendar" | "youtube" | "drive" | "apps" | "routines" | "briefings" | "memory" | "knowhow";

const GROUPS: { items: { page: MenuPage; label: string; icon: string }[] }[] = [
  {
    items: [
      { page: "mail", label: "Mail", icon: "M3 5h18a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm1 2v.5l8 5 8-5V7zm16 2.9-8 5-8-5V17h16z" },
      { page: "calendar", label: "Calendar", icon: "M7 2h2v2h6V2h2v2h3a1 1 0 0 1 1 1v15a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3zM5 9v10h14V9zm2 2h4v4H7z" },
      { page: "youtube", label: "YouTube", icon: "M21.6 7.2a2.5 2.5 0 0 0-1.8-1.8C18.2 5 12 5 12 5s-6.2 0-7.8.4A2.5 2.5 0 0 0 2.4 7.2C2 8.8 2 12 2 12s0 3.2.4 4.8a2.5 2.5 0 0 0 1.8 1.8C5.8 19 12 19 12 19s6.2 0 7.8-.4a2.5 2.5 0 0 0 1.8-1.8c.4-1.6.4-4.8.4-4.8s0-3.2-.4-4.8zM10 15V9l5.2 3z" },
      { page: "drive", label: "Google Drive", icon: "M8.2 3h7.6l6.2 10.7-3.8 6.3H5.8L2 13.7zm.9 2L4.3 13.7 6.6 17.9 11.4 9.6zm6.1 0H10l5.6 9.7h5zM8 16l-.1.1.9 1.9h8.6l1.1-1.9z" },
    ],
  },
  {
    items: [
      { page: "routines", label: "Routines", icon: "M12 3a9 9 0 1 0 9 9h-2a7 7 0 1 1-2.1-5L14 10h7V3l-2.6 2.6A9 9 0 0 0 12 3z" },
      { page: "briefings", label: "Briefings", icon: "M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm0 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14zm1 2v4.6l3.2 1.9-1 1.7L11 12.7V7z" },
      { page: "memory", label: "Memory", icon: "M12 2a7 7 0 0 0-4 12.7V18h8v-3.3A7 7 0 0 0 12 2zm-3 18h6v2H9z" },
      { page: "knowhow", label: "Saved methods", icon: "m12 2 2.9 6.2 6.6.8-4.9 4.6 1.3 6.6L12 16.9l-5.9 3.3 1.3-6.6-4.9-4.6 6.6-.8z" },
    ],
  },
];

/**
 * The row at the bottom of the sidebar: your name, opening a menu with the less-used places
 * (mail, calendar, routines, memory…), document actions, connected apps and Settings.
 */
export default function SidebarMenu({
  name,
  page,
  onGo,
  onNewDocument,
  onOpenFile,
  onSettings,
}: {
  name: string;
  page: string;
  onGo: (page: MenuPage) => void;
  onNewDocument: () => void;
  onOpenFile: () => void;
  onSettings: () => void;
}) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const isMac = navigator.userAgent.includes("Mac");

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => !box.current?.contains(e.target as Node) && setOpen(false);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const pick = (fn: () => void) => () => {
    setOpen(false);
    fn();
  };
  const initials =
    name
      .split(/\s+/)
      .map((w) => w[0])
      .join("")
      .slice(0, 2)
      .toUpperCase() || "J";

  return (
    <div className="sb-account-wrap" ref={box}>
      {open && (
        <div className="sb-menu" role="menu">
          {GROUPS.map((g, i) => (
            <div className="sb-menu-group" key={i}>
              {g.items.map((it) => (
                <button key={it.page} role="menuitem" className={page === it.page ? "on" : ""} onClick={pick(() => onGo(it.page))}>
                  <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d={it.icon} />
                  </svg>
                  {it.label}
                </button>
              ))}
            </div>
          ))}
          <div className="sb-menu-group">
            <button role="menuitem" onClick={pick(onNewDocument)}>
              New document
            </button>
            <button role="menuitem" onClick={pick(onOpenFile)}>
              Open file…
            </button>
          </div>
          <div className="sb-menu-group">
            <button role="menuitem" className={page === "apps" ? "on" : ""} onClick={pick(() => onGo("apps"))}>
              Connected apps
            </button>
            <button role="menuitem" onClick={pick(onSettings)}>
              Settings
              <kbd>{isMac ? "⌘," : "Ctrl ,"}</kbd>
            </button>
          </div>
          <dl className="sb-menu-keys">
            <dt>Talk or mute</dt>
            <dd>⌥ Space</dd>
            <dt>Show or hide Jarvis</dt>
            <dd>⌥ J</dd>
            <dt>Stop speaking</dt>
            <dd>⌥ .</dd>
          </dl>
        </div>
      )}
      <button className={`sb-account${open ? " on" : ""}`} onClick={() => setOpen((o) => !o)} aria-haspopup="menu" aria-expanded={open}>
        <span className="sb-avatar" aria-hidden="true">
          {initials}
        </span>
        <span className="sb-name">{name}</span>
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m7 14 5-5 5 5-1.4 1.4L12 11.8l-3.6 3.6z" />
        </svg>
      </button>
    </div>
  );
}
