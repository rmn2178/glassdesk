const REPOSITORY = "crafter-station/Nosih";
const PAGE_SIZE = 100;
const MAX_PAGES = 100;
const CACHE_CONTROL =
  "public, max-age=3600, s-maxage=3600, stale-while-revalidate=300";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

async function getInstallerDownloadCount(): Promise<number> {
  let total = 0;

  for (let page = 1; page <= MAX_PAGES; page += 1) {
    const url = new URL(`https://api.github.com/repos/${REPOSITORY}/releases`);
    url.searchParams.set("per_page", String(PAGE_SIZE));
    url.searchParams.set("page", String(page));

    const response = await fetch(url, {
      headers: {
        Accept: "application/vnd.github+json",
        "User-Agent": "Nosih-website",
      },
      signal: AbortSignal.timeout(10_000),
    });

    if (!response.ok) {
      throw new Error(`GitHub Releases API returned ${response.status}`);
    }

    const releases: unknown = await response.json();
    if (!Array.isArray(releases)) {
      throw new Error("GitHub Releases API returned an invalid response");
    }

    for (const release of releases) {
      if (!isRecord(release) || !Array.isArray(release.assets)) {
        throw new Error("GitHub Releases API returned an invalid release");
      }

      for (const asset of release.assets) {
        if (!isRecord(asset) || typeof asset.name !== "string") {
          throw new Error("GitHub Releases API returned an invalid asset");
        }

        // Exclude updater manifests and signatures; count downloadable installers only.
        if (!asset.name.toLowerCase().endsWith(".exe")) continue;
        if (
          typeof asset.download_count !== "number" ||
          !Number.isSafeInteger(asset.download_count) ||
          asset.download_count < 0
        ) {
          throw new Error("GitHub Releases API returned an invalid download count");
        }

        total += asset.download_count;
        if (!Number.isSafeInteger(total)) {
          throw new Error("Installer download total exceeds the safe integer range");
        }
      }
    }

    if (releases.length < PAGE_SIZE) return total;
  }

  throw new Error(`GitHub releases exceeded the ${MAX_PAGES}-page limit`);
}

export default {
  async fetch(request: Request): Promise<Response> {
    if (request.method !== "GET") {
      return new Response(null, {
        status: 405,
        headers: { Allow: "GET", "Cache-Control": "no-store" },
      });
    }

    try {
      const downloadCount = await getInstallerDownloadCount();
      return new Response(JSON.stringify({ downloadCount }), {
        headers: {
          "Cache-Control": CACHE_CONTROL,
          "Content-Type": "application/json; charset=utf-8",
        },
      });
    } catch (error) {
      console.error("Could not retrieve Nosih installer download count", error);
      return new Response(
        JSON.stringify({ error: "Download count temporarily unavailable" }),
        {
          status: 502,
          headers: {
            "Cache-Control": "no-store",
            "Content-Type": "application/json; charset=utf-8",
          },
        },
      );
    }
  },
};
