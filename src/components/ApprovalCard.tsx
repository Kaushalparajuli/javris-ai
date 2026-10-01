import { RISK_INFO, useApprovals } from "../lib/approvals";

/** Shows what Jarvis wants to do and waits for a click. One at a time, oldest first. */
export default function ApprovalCard() {
  const { pending, answer } = useApprovals();
  const a = pending[0];
  if (!a) return null;
  return (
    <div className="overlay approval-overlay" role="alertdialog" aria-label="Jarvis needs your approval">
      <div className="approval glass">
        <div className="label">Jarvis wants to · {RISK_INFO[a.risk].label.toLowerCase()}</div>
        <h3>{a.title}</h3>
        {a.detail && <pre className="approval-detail">{a.detail}</pre>}
        {pending.length > 1 && <p className="hint">{pending.length - 1} more waiting after this.</p>}
        <div className="approval-actions">
          <button className="btn" onClick={() => answer(a.id, false)}>
            Reject
          </button>
          <button className="btn primary" autoFocus onClick={() => answer(a.id, true)}>
            {a.okLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
