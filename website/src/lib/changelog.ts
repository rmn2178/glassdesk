import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { site } from "@/data/site";

export interface Release {
  version: string;
  /** ISO date the GitHub release was published, when the API answered at build time. */
  date: string | null;
  url: string;
  /** English bullets, rendered as HTML with inline code. */
  changes: string[];
}

/** The site builds from `nosih/`, so the repository CHANGELOG sits one level up. */
const CHANGELOG_PATH = resolve(process.cwd(), "../CHANGELOG.md");

const escapeHtml = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** The changelog only uses inline code, so that is the only markup rendered. */
const renderInline = (text: string) =>
  escapeHtml(text).replace(/`([^`]+)`/g, "<code>$1</code>");

/** Bullets wrap onto indented lines; join each bullet back into one line. */
function parseBullets(block: string): string[] {
  const items: string[] = [];
  for (const line of block.split("\n")) {
    if (line.startsWith("- ")) items.push(line.slice(2).trim());
    else if (/^\s+\S/.test(line) && items.length) items[items.length - 1] += ` ${line.trim()}`;
  }
  return items.map(renderInline);
}

/** Release dates come from GitHub; the page still builds without them. */
async function fetchReleaseDates(): Promise<Map<string, string>> {
  const dates = new Map<string, string>();
  try {
    const headers: Record<string, string> = { Accept: "application/vnd.github+json" };
    if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
    const api = site.repository.replace("https://github.com/", "https://api.github.com/repos/");
    const response = await fetch(`${api}/releases?per_page=100`, { headers });
    if (!response.ok) return dates;
    const releases = (await response.json()) as { tag_name: string; published_at: string | null }[];
    for (const release of releases) {
      if (release.published_at) dates.set(release.tag_name.replace(/^v/, ""), release.published_at);
    }
  } catch {
    // Offline or rate limited: the page renders versions without dates.
  }
  return dates;
}

/** Published releases, newest first. Unreleased work stays off the page. */
export async function getReleases(): Promise<Release[]> {
  const source = readFileSync(CHANGELOG_PATH, "utf8");
  const dates = await fetchReleaseDates();
  const sections = source.split(/^## \[/m).slice(1);

  return sections
    .map((section) => {
      const version = section.slice(0, section.indexOf("]"));
      const english = section.match(/^### English\n([\s\S]*?)(?=^### |$(?![\s\S]))/m)?.[1] ?? "";
      return {
        version,
        date: dates.get(version) ?? null,
        url: `${site.releases}/tag/v${version}`,
        changes: parseBullets(english),
      };
    })
    .filter((release) => release.version !== "Unreleased" && release.changes.length > 0);
}
