import { site } from "@/data/site";

export const signingCredit = "Free code signing provided by SignPath.io, certificate by SignPath Foundation.";

export const signingTeam = {
  committers: [{ name: "CristianOlivera1", href: "https://github.com/CristianOlivera1" }],
  reviewers: [{ name: "CristianOlivera1", href: "https://github.com/CristianOlivera1" }],
  approvers: [{ name: "CristianOlivera1", href: "https://github.com/CristianOlivera1" }],
} as const;

export const updateManifest = `${site.releases}/latest/download/latest.json`;
