/** Expand and close buttons shared by everything that opens in the side panel. */
export default function PanelControls({ expanded, onToggleExpand, onClose }: { expanded: boolean; onToggleExpand: () => void; onClose: () => void }) {
  return (
    <div className="panel-controls">
      <button
        className="icon-btn"
        onClick={onToggleExpand}
        aria-label={expanded ? "Show the conversation again" : "Expand to full width"}
        title={expanded ? "Show the conversation" : "Expand"}
      >
        <svg viewBox="0 0 24 24">
          {expanded ? (
            <path d="M10 4v6H4V8h2.6L3.3 4.7l1.4-1.4L8 6.6V4zm4 16v-6h6v2h-2.6l3.3 3.3-1.4 1.4-3.3-3.3V20z" />
          ) : (
            <path d="M4 4h6v2H7.4l3.3 3.3-1.4 1.4L6 7.4V10H4zm16 16h-6v-2h2.6l-3.3-3.3 1.4-1.4 3.3 3.3V14h2z" />
          )}
        </svg>
      </button>
      <button className="icon-btn" onClick={onClose} aria-label="Close" title="Close">
        <svg viewBox="0 0 24 24">
          <path d="M6.4 5 5 6.4 10.6 12 5 17.6 6.4 19 12 13.4 17.6 19 19 17.6 13.4 12 19 6.4 17.6 5 12 10.6z" />
        </svg>
      </button>
    </div>
  );
}
