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

/**
 * Extra rules for languages the voice model tends to get wrong. Nepali and Hindi share a script and
 * many words, and people mix them with English, so each gets its own guide.
 */
export function languageGuide(code: string): string {
  if (code === "ne")
    return `NEPALI: Reply in Nepali (नेपाली), written in Devanagari and spoken naturally as people in Nepal speak it: polite "तपाईं" forms ("गर्नुहोस्", "हुन्छ", "छ", "हो"), never Hindi forms like "है", "करो", "आप", "क्या". Everyday English words (app, email, file, project) may stay in English, the way Nepali speakers mix them. If ${"{name}"} writes or says Nepali in Roman letters, understand it and still answer in Devanagari. Read numbers, dates and times the Nepali way ("बिहान ८ बजे").`;
  if (code === "hi")
    return `HINDI: Reply in Hindi (हिन्दी), written in Devanagari, in everyday spoken Hindi (Hindustani) rather than formal Sanskritised Hindi: "है", "करो/कीजिए", "आप". Keep common English words (app, email, file, project) in English, the way Hindi speakers mix them. If ${"{name}"} writes Hindi in Roman letters (Hinglish), understand it and answer in Devanagari. It is not Nepali: no "छ", "गर्नुहोस्" or "तपाईं".`;
  return "";
}

/** Display name for a stored code; empty code means "match whoever is talking". */
export function languageName(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.name ?? "";
}
