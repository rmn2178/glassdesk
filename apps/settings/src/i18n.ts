import type { Language } from "./config";
import en from "./i18n/en.json";
import es from "./i18n/es.json";
import fr from "./i18n/fr.json";
import zh from "./i18n/zh.json";
import hi from "./i18n/hi.json";
import ar from "./i18n/ar.json";
import bn from "./i18n/bn.json";
import pt from "./i18n/pt.json";

export type Texts = typeof en;
export type ResolvedLanguage = Exclude<Language, "auto">;
const TEXTS: Record<ResolvedLanguage, Texts> = { en, es, fr, zh, hi, ar, bn, pt };

/** Autonyms keep the language picker usable after an accidental selection. */
export const LANGUAGE_OPTIONS: readonly { value: ResolvedLanguage; label: string }[] = [
  { value: "en", label: "English" },
  { value: "es", label: "Español" },
  { value: "fr", label: "Français" },
  { value: "zh", label: "简体中文" },
  { value: "hi", label: "हिन्दी" },
  { value: "ar", label: "العربية" },
  { value: "bn", label: "বাংলা" },
  { value: "pt", label: "Português" },
];

export function resolveLanguage(language: Language, systemLanguage: string): ResolvedLanguage {
  if (language !== "auto") return language;
  const primary = systemLanguage.toLowerCase().split(/[-_]/)[0];
  return Object.hasOwn(TEXTS, primary) ? primary as ResolvedLanguage : "en";
}

export function textsFor(language: ResolvedLanguage): Texts {
  return TEXTS[language];
}
