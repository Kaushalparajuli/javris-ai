// Languages Jarvis can be told to speak by default.
//
// Native-audio Live models pick the language themselves and reject an explicit
// speechConfig.languageCode, so the choice is applied through the system prompt.
// The codes are kept anyway: they are stable, and they are what a future
// languageCode-capable model would want.

export interface Language {
  code: string;
  name: string;
}

export const LANGUAGES: Language[] = [
  { code: "en", name: "English" },
  { code: "ne", name: "Nepali" },
  { code: "hi", name: "Hindi" },
  { code: "ar", name: "Arabic" },
  { code: "bn", name: "Bengali" },
  { code: "de", name: "German" },
  { code: "es", name: "Spanish" },
  { code: "fa", name: "Persian" },
  { code: "fr", name: "French" },
  { code: "gu", name: "Gujarati" },
  { code: "he", name: "Hebrew" },
  { code: "id", name: "Indonesian" },
  { code: "it", name: "Italian" },
  { code: "ja", name: "Japanese" },
  { code: "ko", name: "Korean" },
  { code: "mr", name: "Marathi" },
  { code: "ms", name: "Malay" },
  { code: "nl", name: "Dutch" },
  { code: "pa", name: "Punjabi" },
  { code: "pl", name: "Polish" },
  { code: "pt", name: "Portuguese" },
  { code: "ru", name: "Russian" },
  { code: "sw", name: "Swahili" },
  { code: "ta", name: "Tamil" },
  { code: "te", name: "Telugu" },
  { code: "th", name: "Thai" },
  { code: "tl", name: "Filipino" },
  { code: "tr", name: "Turkish" },
  { code: "uk", name: "Ukrainian" },
  { code: "ur", name: "Urdu" },
  { code: "vi", name: "Vietnamese" },
  { code: "zh-Hans", name: "Chinese (Simplified)" },
];

/** Display name for a stored code; empty code means "match whoever is talking". */
export function languageName(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.name ?? "";
}
