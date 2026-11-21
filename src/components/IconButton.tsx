// Small icon buttons for row and toolbar actions (edit, delete, show in Finder…), the way macOS
// apps do it: an icon, with the words as a tooltip and for screen readers.

import type { ButtonHTMLAttributes } from "react";

const ICONS = {
  edit: "M4 17.2V20h2.8l8.3-8.3-2.8-2.8zm14.7-7.5a1 1 0 0 0 0-1.4l-2-2a1 1 0 0 0-1.4 0l-1.5 1.5 2.8 2.8z",
  delete: "M9 3h6l1 2h4v2H4V5h4zm-3 6h12l-1 12H7zm4 2v8h2v-8zm4 0v8h-2v-8z",
  remove: "M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4z",
  newChat: "M4 4h16a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H8l-4 4V5a1 1 0 0 1 1-1zm7 3v3H8v2h3v3h2v-3h3v-2h-3V7z",
  finder: "M3 6a1 1 0 0 1 1-1h6l2 2h8a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
  copy: "M8 3h10a2 2 0 0 1 2 2v12h-2V5H8zM5 7h10a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1zm1 2v10h8V9z",
  open: "M14 3h7v7h-2V6.4l-7.3 7.3-1.4-1.4L17.6 5H14zM5 5h6v2H6v11h11v-5h2v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z",
} as const;

export type IconName = keyof typeof ICONS;

export default function IconButton({
  icon,
  label,
  danger,
  className,
  ...rest
}: { icon: IconName; label: string; danger?: boolean } & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button type="button" className={`icon-action${danger ? " danger" : ""}${className ? ` ${className}` : ""}`} title={label} aria-label={label} {...rest}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d={ICONS[icon]} />
      </svg>
    </button>
  );
}
