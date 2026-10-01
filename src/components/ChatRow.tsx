import { useEffect, useRef, useState } from "react";
import { day } from "../lib/format";
import type { ChatSummary, Workspace } from "../lib/types";

/** One chat in the sidebar: click to open, ⋯ to rename, pin, move to a project or delete, or drag it onto a project. */
export default function ChatRow({
  chat,
  active,
  projects,
  onOpen,
  onRename,
  onPin,
  onMove,
  onDelete,
}: {
  chat: ChatSummary;
  active: boolean;
  projects: Workspace[];
  onOpen: () => void;
  onRename: (title: string) => void;
  onPin: (pinned: boolean) => void;
  onMove: (slug: string) => void;
  onDelete: () => void;
}) {
  const [menu, setMenu] = useState<"main" | "move" | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const box = useRef<HTMLDivElement>(null);
  const title = chat.title || "New chat";

  useEffect(() => {
    if (!menu) return;
    const away = (e: MouseEvent) => !box.current?.contains(e.target as Node) && setMenu(null);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setMenu(null);
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
    };
  }, [menu]);

  const finish = (save: boolean) => {
    setEditing(false);
    if (save && draft.trim() !== chat.title) onRename(draft.trim());
  };

  if (editing)
    return (
      <div className="chat-edit">
        <input
          autoFocus
          value={draft}
          placeholder="Chat name (empty = automatic)"
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => finish(true)}
          onKeyDown={(e) => {
            if (e.key === "Enter") finish(true);
            else if (e.key === "Escape") finish(false);
          }}
        />
      </div>
    );

  return (
    <div className="chat-row" ref={box}>
      <button
        className={active ? "on" : ""}
        draggable
        onDragStart={(e) => {
          e.dataTransfer.setData("application/x-jarvis-chat", chat.id);
          e.dataTransfer.effectAllowed = "move";
        }}
        onClick={onOpen}
        onDoubleClick={() => {
          setDraft(chat.title);
          setEditing(true);
        }}
        title={title}
      >
        {chat.pinned && (
          <svg className="pin" viewBox="0 0 24 24" aria-label="Pinned">
            <path d="M14 3 21 10l-2 1-3 3 .5 4.5-1.5 1.5-3.5-3.5L6 21l-1-1 4.5-4.5L6 12l1.5-1.5L12 11l3-3z" />
          </svg>
        )}
        <b>{title}</b>
        <span>{day(chat.updatedAt)}</span>
        <i
          role="button"
          aria-label={`Options for ${title}`}
          title="Rename, pin, move…"
          onClick={(e) => {
            e.stopPropagation();
            setMenu(menu ? null : "main");
          }}
        >
          ⋯
        </i>
      </button>
      {menu && (
        <div className="chat-menu" role="menu">
          {menu === "main" ? (
            <>
              <button
                role="menuitem"
                onClick={() => {
                  setMenu(null);
                  setDraft(chat.title);
                  setEditing(true);
                }}
              >
                Rename
              </button>
              <button role="menuitem" onClick={() => { setMenu(null); onPin(!chat.pinned); }}>
                {chat.pinned ? "Unpin" : "Pin to top"}
              </button>
              <button role="menuitem" onClick={() => setMenu("move")}>
                Move to project…
              </button>
              <button role="menuitem" className="danger" onClick={() => { setMenu(null); onDelete(); }}>
                Delete
              </button>
            </>
          ) : (
            <>
              <button role="menuitem" className={!chat.workspace ? "on" : ""} onClick={() => { setMenu(null); onMove(""); }}>
                No project
              </button>
              {projects.map((p) => (
                <button key={p.slug} role="menuitem" className={chat.workspace === p.slug ? "on" : ""} onClick={() => { setMenu(null); onMove(p.slug); }}>
                  {p.name}
                </button>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}
