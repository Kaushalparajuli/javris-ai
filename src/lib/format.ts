/** "today", "yesterday", or a short day/month for anything older. */
export function day(ms: number) {
  const diff = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(ms).setHours(0, 0, 0, 0)) / 86400000);
  if (diff === 0) return "today";
  if (diff === 1) return "yesterday";
  return new Date(ms).toLocaleDateString([], { day: "numeric", month: "short" });
}
