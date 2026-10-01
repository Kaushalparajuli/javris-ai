// A short Gemini text call that names a chat after what it is about, in the language it's held in.

import { API, pickModels } from "./routines";
import type { Msg } from "./types";

const RULES = `You name conversations. Read the conversation between a person and their assistant Jarvis and write a short title for what it is about.

- 2 to 6 words, naming the main topic, not "Chat" or "Conversation".
- Use the language the person is speaking, in its own script (Nepali and Hindi in Devanagari).
- If the topic has changed during the conversation, name the topic being discussed most recently.
- No quotes, no trailing punctuation, no emoji.
Reply with the title only.`;

/** A title for the conversation, or "" if it can't be made. */
export async function titleForChat(apiKey: string, messages: Msg[], current = ""): Promise<string> {
  const talk = messages.filter((m) => m.who === "you" || m.who === "jarvis");
  if (!apiKey || talk.length < 2) return "";
  const transcript = talk
    .slice(-20)
    .map((m) => `${m.who === "you" ? "Person" : "Jarvis"}: ${m.text.slice(0, 400)}`)
    .join("\n");
  const body = JSON.stringify({
    systemInstruction: { parts: [{ text: RULES }] },
    contents: [{ role: "user", parts: [{ text: `${current ? `Current title: ${current}\n\n` : ""}${transcript}` }] }],
    generationConfig: { temperature: 0.2, maxOutputTokens: 40 },
  });
  for (const model of await pickModels(apiKey)) {
    const res = await fetch(`${API}/models/${model}:generateContent?key=${encodeURIComponent(apiKey)}`, { method: "POST", headers: { "Content-Type": "application/json" }, body });
    if (!res.ok) {
      if ([404, 429, 500, 503].includes(res.status)) continue;
      return "";
    }
    const data = await res.json();
    const text: string = data.candidates?.[0]?.content?.parts?.map((p: { text?: string }) => p.text ?? "").join("") ?? "";
    const title = text.split("\n")[0].replace(/^["'“”‘’\s]+|["'“”‘’\s.。]+$/g, "").trim();
    return title.length >= 2 ? title.slice(0, 60) : "";
  }
  return "";
}
