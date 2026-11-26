import claude from "@/assets/usage/claude.svg";
import codex from "@/assets/usage/codex.svg";
import gemini from "@/assets/usage/gemini.svg";
import opencode from "@/assets/usage/opencode.svg";

export interface QuotaExample {
  label: string;
  used: number;
  resets: string;
}

/** One supported AI client with illustrative, not live, quota values. */
export interface UsageProvider {
  name: string;
  logo: ImageMetadata;
  color: string;
  primary: QuotaExample;
  secondary?: QuotaExample;
}

export const usageProviders: UsageProvider[] = [
  {
    name: "Claude Code",
    logo: claude,
    color: "#d97757",
    primary: { label: "5 h", used: 46, resets: "Resets 3:10 PM" },
    secondary: { label: "7 d", used: 35, resets: "Resets Nov 21" },
  },
  {
    name: "Codex",
    logo: codex,
    color: "#87bef5",
    primary: { label: "5 h", used: 28, resets: "Resets 4:30 PM" },
    secondary: { label: "7 d", used: 41, resets: "Resets Nov 21" },
  },
  {
    name: "OpenCode Go",
    logo: opencode,
    color: "#84c4a6",
    primary: { label: "5 h", used: 25, resets: "Resets 4:00 PM" },
    secondary: { label: "7 d", used: 40, resets: "Resets Nov 21" },
  },
  {
    name: "Gemini / Antigravity",
    logo: gemini,
    color: "#6f92ff",
    primary: { label: "Gemini Pro", used: 49, resets: "Resets 5:10 PM" },
    secondary: { label: "Gemini Flash", used: 18, resets: "Resets Nov 21" },
  },
];

export const usageIntro = {
  eyebrow: "AI usage",
  title: "AI plan usage, at a glance.",
  lead: "Available quotas and reset times for Claude Code, Codex, OpenCode Go, Gemini / Antigravity and Z.ai GLM Coding Plan, in the menu bar.",
} as const;

export const usageFacts = [
  {
    title: "Only while you use them",
    body: "Providers appear while a supported client is running. Z.ai works with OpenCode and your configured Coding Plan key.",
  },
  {
    title: "Your existing session",
    body: "Nosih uses the sign-in or Coding Plan key already configured on your PC. Gemini Pro and Flash stay separate when those quotas are available.",
  },
  {
    title: "Straight to the provider",
    body: "It checks available limits every five minutes or when you press Refresh. Antigravity is queried through its authenticated local service; a separate process keeps the dock responsive.",
  },
  {
    title: "One switch to turn it off",
    body: "Settings → Menu bar → Show AI provider usage. Nosih stores no credentials.",
  },
] as const;
