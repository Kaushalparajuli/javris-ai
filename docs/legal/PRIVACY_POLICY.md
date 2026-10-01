# Jarvis Privacy Policy

**Effective date:** October 1, 2026
**Developer:** GSoft Technologies ("we", "us")
**Contact:** kaushal123parajuli@gmail.com

Jarvis is a voice research assistant for macOS. This policy explains what information Jarvis handles, where it goes, and the choices you have.

## The short version

- Jarvis runs on your Mac. **We do not operate servers for Jarvis, and we do not receive, store, or sell your data.**
- Jarvis contains **no analytics, tracking, advertising, or telemetry**.
- To work, Jarvis sends some of your data to services **you connect with your own accounts**: Google Gemini (voice and AI), OpenAI Codex via your ChatGPT account (research and tasks), and optionally Google Gmail and Calendar. Those providers handle that data under their own privacy policies.
- Everything Jarvis saves (chats, reports, notes, settings) stays in folders on your Mac that you can open, edit, or delete.

## 1. Information Jarvis handles

### 1.1 Voice and microphone audio
- When you start a voice session (mic button, ⌥Space, or the wake word), Jarvis records audio from your microphone and streams it to the **Google Gemini Live API** so Gemini can understand and answer you. Gemini's spoken replies and live transcripts are sent back to the app.
- **Wake word ("Hey Jarvis")**, if you turn it on: the microphone is checked on your Mac in short slices, using a model built into the app. This audio is **processed only on your device, is not recorded, and is not sent anywhere**. Only after the wake phrase is detected does a normal voice session begin.
- macOS asks for microphone permission before Jarvis can use the mic. You can withdraw it at any time in System Settings → Privacy & Security → Microphone.

### 1.2 Text, files, and images you provide
Messages you type, images you attach or drag in, and documents you import are sent to Google Gemini and/or the Codex worker as needed to complete your request.

### 1.3 On-screen context (only when used)
When you ask Jarvis to "explain this", "rewrite this", or similar, Jarvis may read:
- the name of the frontmost app and its window title, and
- the text you have selected, using the macOS Accessibility API (or, as a fallback, by briefly copying the selection with ⌘C and then restoring your clipboard).

This is read **only on request**, never continuously, and is sent to the AI service handling that request. It requires the macOS Accessibility permission, which you can revoke at any time.

### 1.4 Research, tasks, and the browser
- Research, image, document and code tasks are carried out by the **OpenAI Codex CLI**, signed in with **your own ChatGPT account**. Your request, relevant context, attached files, and (for code tasks) the contents of the project folder you choose are processed by OpenAI under your ChatGPT account.
- Codex may search the web and visit websites. When the browser agent is used, a separate Chrome profile controlled by Jarvis visits sites on your behalf; those websites receive normal browsing information (such as your IP address) and may set cookies in that profile.

### 1.5 Google account data (optional)
If you connect Google, Jarvis requests these permissions:

| Permission | What Jarvis uses it for |
|---|---|
| `gmail.readonly` | Search and read your emails when you ask (e.g., "summarize today's mail"), and as input to routines you set up |
| `gmail.compose` | Create drafts and send emails **only after you approve each send** |
| `calendar.events` | List your events and create events or invites **after you approve** |
| Basic profile (email address) | Show which account is connected |

Email and calendar content that Jarvis reads is sent to Google Gemini and/or the Codex worker only to complete the task you asked for.

**Google API Services User Data Policy.** Jarvis's use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements. Specifically, Google user data is:
- used only to provide the features you request in Jarvis;
- not transferred to anyone except as needed to provide those features (the AI services described above), to comply with law, or with your consent;
- never used for advertising, never sold, and never used to build user profiles; and
- never read by humans at GSoft Technologies, except with your explicit permission for a specific purpose (such as support you request), for security purposes, or to comply with law.
- not used to develop, improve, or train generalized AI or machine-learning models.

### 1.6 Settings and credentials
Your Gemini API key, preferences, and approval rules are stored in a settings file on your Mac (`~/Library/Application Support/co.fikraventures.jarvis/`) readable only by your user account. Google sign-in tokens are stored in the same app folder. Codex stores its own ChatGPT sign-in, managed by the Codex CLI.

## 2. Where your data is stored

Jarvis keeps its data locally, by default under `~/Jarvis/`:

| Data | Location |
|---|---|
| Research reports and summaries | `~/Jarvis/research/` |
| Chats and transcripts | `~/Jarvis/chats/` |
| Task history | `~/Jarvis/tasks.json` |
| Notes ("remember that…") | `~/Jarvis/notes.md` |
| Routines and their results | `~/Jarvis/routines*` |
| Know-how (saved skills) | `~/Jarvis/skills/` |
| Approval / action log | `~/Jarvis/audit.jsonl` |
| Diagnostic logs | `~/Jarvis/logs/` (on your Mac only, never uploaded) |
| Browser agent profile | `~/Jarvis/.browser-profile/` |

Your notes and know-how may be included in instructions sent to the AI services so Jarvis can use them.

## 3. Third-party services

Jarvis connects directly from your Mac to these services, using accounts and keys you provide. Your use of them is governed by their terms and privacy policies, including how long they keep data and whether they use it to improve their models:

- **Google Gemini API** — [Gemini API Additional Terms](https://ai.google.dev/gemini-api/terms) and [Google Privacy Policy](https://policies.google.com/privacy). Note: on Google's **unpaid** API tier, Google may use your inputs and outputs to improve its products, and human reviewers may read them. Use a paid tier if this matters to you.
- **OpenAI (Codex / ChatGPT)** — [OpenAI Privacy Policy](https://openai.com/policies/privacy-policy). Your ChatGPT data controls apply.
- **Google Gmail and Calendar** — [Google Privacy Policy](https://policies.google.com/privacy).
- **Websites** visited by the research worker or browser agent — each site's own policy.

We do not control these services and are not responsible for their practices.

## 4. What we do not do
- We do not collect your data on our servers.
- We do not sell or rent personal information.
- We do not use analytics, ad networks, or tracking SDKs.
- We do not use your data to train AI models.

## 5. Your choices and controls
- **Approvals:** sending email, creating invites, and changing files ask for your OK by default. You can adjust these rules in Settings.
- **Disconnect Google** in Settings, or at any time at [myaccount.google.com/permissions](https://myaccount.google.com/permissions).
- **Sign out of Codex** with `codex logout`.
- **Remove your Gemini API key** in Settings, or revoke it in Google AI Studio.
- **Turn off** the wake word, notifications, or web search in Settings.
- **Revoke** microphone or Accessibility permissions in macOS System Settings.
- **Delete your data:** delete the `~/Jarvis/` folder and `~/Library/Application Support/co.fikraventures.jarvis/`. Data already sent to third-party services must be deleted through those services.

## 6. Security
Data is stored on your Mac and protected by your macOS account. Connections to third-party services use HTTPS/TLS. Settings are stored with permissions that restrict access to your user. No method of storage or transmission is completely secure; please keep your Mac and accounts protected.

## 7. Children
Jarvis is not intended for children under 13 (or the minimum age required in your country). We do not knowingly process children's data.

## 8. Your rights
Because we do not hold your personal data, most requests (access, correction, deletion) can be completed directly on your Mac as described above. For data held by Google or OpenAI, contact them directly. If you have questions, contact us at kaushal123parajuli@gmail.com.

## 9. Changes to this policy
We may update this policy as Jarvis changes. We will update the effective date above and, for significant changes, let you know in the app or release notes.

## 10. Contact
GSoft Technologies
Kathmandu, Nepal
kaushal123parajuli@gmail.com
