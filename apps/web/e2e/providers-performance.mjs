import { chromium } from "playwright";

const baseUrl = process.env.E2E_BASE_URL ?? "https://web.vibe-plus.localhost";
const budgetMs = Number(process.env.PROVIDERS_E2E_BUDGET_MS ?? 1500);
const timeoutMs = Number(process.env.PROVIDERS_E2E_TIMEOUT_MS ?? 15000);
const headless = process.env.HEADED !== "1";

async function getProvidersState(page) {
  return page.locator('[data-testid="providers-complete"]').evaluate((el) => ({
    providerCount: Number(el.getAttribute("data-provider-count") ?? 0),
    credentialCount: Number(el.getAttribute("data-credential-count") ?? 0),
    textLen: document.body.innerText.length,
  }));
}

const browser = await chromium.launch({ headless });
const page = await browser.newPage({
  ignoreHTTPSErrors: true,
  viewport: { width: 1440, height: 1000 },
});

try {
  const startedAt = performance.now();
  await page.goto(`${baseUrl}/ui/providers`, { waitUntil: "commit", timeout: timeoutMs });

  await page
    .locator('[data-testid="providers-complete"]')
    .waitFor({ state: "visible", timeout: timeoutMs });

  const readyMs = performance.now() - startedAt;
  const summary = await getProvidersState(page);

  if (summary.providerCount <= 0) {
    throw new Error(`Providers data did not complete. summary=${JSON.stringify(summary)}`);
  }

  if (readyMs > budgetMs) {
    throw new Error(
      `Providers complete data took ${readyMs.toFixed(1)}ms, over ${budgetMs}ms budget. summary=${JSON.stringify(summary)}`,
    );
  }

  console.log(
    JSON.stringify(
      {
        page: "/ui/providers",
        readyMs: Math.round(readyMs),
        budgetMs,
        summary,
      },
      null,
      2,
    ),
  );
} finally {
  await browser.close();
}
