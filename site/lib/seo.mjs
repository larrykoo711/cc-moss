// What search engines read: head tags, structured data, sitemap, robots.txt. Every claim here is
// built from something the page shows or the repo records; nothing is declared for show.
import { HTML_LANG, LOCALES, ORIGIN, REPO, VERSION, localePath, url } from "./site.mjs";

const OG_LOCALE = { en: "en_US", zh: "zh_CN" };
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
/** JSON in a <script> must not be able to close it. */
const ld = (data) => `  <script type="application/ld+json">${JSON.stringify(data).replace(/</g, "\\u003c")}</script>`;

export const twinPath = (path) => (path.endsWith("/") ? `${path}index.md` : `${path}.md`);

/** <title>, description, canonical, hreflang, Open Graph, Twitter and JSON-LD for one page. */
export function headTags({ page, locale, updated, faq = [] }) {
  const path = localePath(locale, page.path);
  const title = page.title[locale];
  const description = page.description[locale];
  const image = url(`/og/${page.id}.${locale}.png`);
  const alt = LOCALES.map((l) => `  <link rel="alternate" hreflang="${l === "zh" ? "zh" : "en"}" href="${url(localePath(l, page.path))}">`);
  const other = LOCALES.find((l) => l !== locale);
  return [
    `  <title>${esc(title)}</title>`,
    `  <meta name="description" content="${esc(description)}">`,
    `  <link rel="canonical" href="${url(path)}">`,
    ...alt,
    `  <link rel="alternate" hreflang="x-default" href="${url(localePath("en", page.path))}">`,
    `  <link rel="alternate" type="text/markdown" href="${url(twinPath(path))}">`,
    `  <meta http-equiv="content-language" content="${HTML_LANG[locale]}">`,
    `  <meta property="og:type" content="${page.id === "home" ? "website" : "article"}">`,
    `  <meta property="og:site_name" content="cc-moss">`,
    `  <meta property="og:title" content="${esc(title)}">`,
    `  <meta property="og:description" content="${esc(description)}">`,
    `  <meta property="og:url" content="${url(path)}">`,
    `  <meta property="og:image" content="${image}">`,
    `  <meta property="og:image:width" content="1200">`,
    `  <meta property="og:image:height" content="630">`,
    `  <meta property="og:image:alt" content="${esc(title)}">`,
    `  <meta property="og:locale" content="${OG_LOCALE[locale]}">`,
    `  <meta property="og:locale:alternate" content="${OG_LOCALE[other]}">`,
    `  <meta name="twitter:card" content="summary_large_image">`,
    `  <meta name="twitter:title" content="${esc(title)}">`,
    `  <meta name="twitter:description" content="${esc(description)}">`,
    `  <meta name="twitter:image" content="${image}">`,
    ld(structuredData({ page, locale, path, updated, faq, image })),
  ].join("\n");
}

const AUTHOR = { "@type": "Organization", "@id": `${ORIGIN}/#contributors`, name: "cc-moss contributors", url: REPO };

/** schema.org graph for a page. FAQPage is built from the FAQ the page actually renders. */
export function structuredData({ page, locale, path, updated, faq, image }) {
  const inLanguage = HTML_LANG[locale];
  const webpage = {
    "@type": "WebPage",
    "@id": `${url(path)}#page`,
    url: url(path),
    name: page.title[locale],
    description: page.description[locale],
    inLanguage,
    dateModified: updated,
    isPartOf: { "@id": `${ORIGIN}/#website` },
    primaryImageOfPage: image,
  };
  const graph = [
    { "@type": "WebSite", "@id": `${ORIGIN}/#website`, url: `${ORIGIN}/`, name: "cc-moss", inLanguage: LOCALES.map((l) => HTML_LANG[l]), publisher: { "@id": AUTHOR["@id"] } },
    AUTHOR,
    webpage,
  ];
  // The app itself is described where the page shows it: the home page.
  if (page.id === "home") {
    graph.push(
      {
        "@type": "SoftwareApplication",
        "@id": `${ORIGIN}/#app`,
        name: "cc-moss",
        description: page.description[locale],
        applicationCategory: "DeveloperApplication",
        applicationSubCategory: "Session manager for Claude Code",
        operatingSystem: "macOS 14 or later, Linux",
        softwareVersion: VERSION,
        url: `${ORIGIN}/`,
        downloadUrl: `${REPO}/releases/latest`,
        installUrl: url(localePath(locale, "/#install")),
        softwareHelp: url(localePath(locale, "/#faq")),
        license: "https://opensource.org/license/mit",
        isAccessibleForFree: true,
        offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
        screenshot: url(`/assets/panel.${locale}.dark.webp`),
        author: { "@id": AUTHOR["@id"] },
      },
      {
        "@type": "SoftwareSourceCode",
        "@id": `${ORIGIN}/#source`,
        name: "cc-moss",
        codeRepository: REPO,
        programmingLanguage: ["Rust", "Swift"],
        license: "https://opensource.org/license/mit",
        targetProduct: { "@id": `${ORIGIN}/#app` },
      },
    );
    if (faq.length)
      graph.push({
        "@type": "FAQPage",
        "@id": `${url(path)}#faq`,
        inLanguage,
        mainEntity: faq.map(({ q, a }) => ({ "@type": "Question", name: q, acceptedAnswer: { "@type": "Answer", text: a } })),
      });
  }
  return { "@context": "https://schema.org", "@graph": graph };
}

/** One <url> per page per language, each naming every language version (and x-default). */
export function sitemap(entries) {
  const urls = entries.map(({ page, locale, updated }) => {
    const alts = LOCALES.map((l) => `    <xhtml:link rel="alternate" hreflang="${l}" href="${url(localePath(l, page.path))}"/>`);
    return [
      "  <url>",
      `    <loc>${url(localePath(locale, page.path))}</loc>`,
      `    <lastmod>${updated}</lastmod>`,
      ...alts,
      `    <xhtml:link rel="alternate" hreflang="x-default" href="${url(localePath("en", page.path))}"/>`,
      "  </url>",
    ].join("\n");
  });
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n${urls.join("\n")}\n</urlset>\n`;
}

/** Search and AI answers welcome; training is declined through Content-Signal, not by blocking. */
export function robots() {
  return [
    "# cc-moss.larrykoo.com",
    "# Search engines and AI assistants are welcome to read and cite this site.",
    "# Content-Signal states our preference on model training (https://contentsignals.org).",
    "",
    "User-agent: *",
    "Content-Signal: search=yes, ai-input=yes, ai-train=no",
    "Allow: /",
    "",
    `Sitemap: ${ORIGIN}/sitemap.xml`,
    "",
  ].join("\n");
}
