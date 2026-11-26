import { site } from "@/data/site";

export const navigation = [
  { label: "Dock", href: "/#dock" },
  { label: "AI usage", href: "/#ai-usage" },
  { label: "Mascot", href: "/#crab" },
  { label: "Settings", href: "/#customize" },
  { label: "Reversible", href: "/#safety" },
  { label: "About", href: "/about" },
] as const;

export const footerGroups = [
  {
    title: "Product",
    links: [
      { label: "The Dock", href: "/#dock", external: false },
      { label: "AI usage", href: "/#ai-usage", external: false },
      { label: "Mascot", href: "/#crab", external: false },
      { label: "Settings", href: "/#customize", external: false },
      { label: "Reversible", href: "/#safety", external: false },
      { label: "Download", href: "/#download", external: false },
    ],
  },
  {
    title: "Source",
    links: [
      { label: "GitHub", href: site.repository, external: true },
      { label: "Releases", href: site.releases, external: true },
      { label: "Issues", href: site.issues, external: true },
    ],
  },
  {
    title: "Project",
    links: [
      { label: "License", href: `${site.repository}/blob/main/LICENSE`, external: true },
      { label: "Changelog", href: "/changelog", external: false },
      { label: "About", href: "/about", external: false },
      { label: "Contribute", href: site.issues, external: true },
      { label: "Code signing policy", href: "/code-signing-policy", external: false },
      { label: "Privacy", href: "/privacy", external: false },
    ],
  },
] as const;
