// UI end-to-end test against the real built app (Tauri + WebView2), driven through the Chrome DevTools
// Protocol. Usage: node e2e/run.mjs <app.exe> <data-dir> [--screenshots <dir>]
// The app is started with CONTEXTFORGE_HOME=<data-dir> so it never touches the user's real data.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const [appExe, dataDir] = process.argv.slice(2);
const shotIdx = process.argv.indexOf("--screenshots");
const shots = shotIdx > 0 ? process.argv[shotIdx + 1] : null;
if (!appExe || !dataDir) {
  console.error("usage: node e2e/run.mjs <app.exe> <data-dir> [--screenshots <dir>]");
  process.exit(2);
}
const port = 9300 + Math.floor(Math.random() * 400);
const app = spawn(appExe, [], {
  env: { ...process.env, CONTEXTFORGE_HOME: dataDir, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
  stdio: "ignore",
});

const results = [];
let currentPage = null;
async function step(name, fn) {
  const t = Date.now();
  try {
    if (currentPage) { await currentPage.keyboard.press("Escape").catch(() => {}); await currentPage.keyboard.press("Escape").catch(() => {}); }
    await fn();
    results.push({ name, ok: true, ms: Date.now() - t });
    console.log(`  ok   ${name}`);
  } catch (e) {
    results.push({ name, ok: false, ms: Date.now() - t, error: String(e?.message ?? e) });
    console.log(`  FAIL ${name}: ${String(e?.message ?? e).split("\n")[0]}`);
    if (currentPage) { mkdirSync("e2e/failures", { recursive: true }); await currentPage.screenshot({ path: `e2e/failures/${name.replace(/[^a-z0-9]+/gi, "-")}.png` }).catch(() => {}); }
  }
}

let browser;
try {
  for (let i = 0; i < 60 && !browser; i++) {
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    } catch {
      await new Promise((r) => setTimeout(r, 500));
    }
  }
  if (!browser) throw new Error("could not attach to the app's WebView2");
  const page = browser.contexts()[0].pages()[0];
  currentPage = page;
  const main = page.locator("main");
  await page.setViewportSize?.({ width: 1440, height: 900 }).catch(() => {});
  const shot = async (name) => {
    if (!shots) return;
    mkdirSync(shots, { recursive: true });
    await page.locator('[aria-live="polite"] > div').first().waitFor({ state: "detached", timeout: 9000 }).catch(() => {});
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(shots, `${name}.png`) });
  };
  const nav = (label) => page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: new RegExp(`^${label}`) }).click();
  const heading = (text) => page.getByRole("heading", { level: 1, name: text }).waitFor({ timeout: 10000 });

  await step("app starts and renders the overview with real data", async () => {
    await page.getByRole("navigation", { name: "Main" }).waitFor({ timeout: 20000 });
    await nav("Overview");
    await heading("Overview");
    await main.getByText("Memories", { exact: true }).first().waitFor();
    await main.getByText("Needs review", { exact: true }).first().waitFor();
    await main.getByText("Claude Code").first().waitFor();
    await shot("overview");
  });
  await step("projects screen shows the registered Git project", async () => {
    await nav("Projects");
    await heading("Projects");
    await main.getByText("acme-shop").first().waitFor();
    await main.getByText("Git", { exact: true }).first().waitFor();
    await shot("projects");
  });
  await step("agent context preview opens", async () => {
    await page.getByRole("button", { name: "Preview agent context" }).click();
    await page.getByText("<contextforge-memory").waitFor();
    await page.keyboard.press("Escape");
  });
  await step("memory explorer search and provenance", async () => {
    await nav("Memory");
    await heading("Memory");
    await page.getByRole("textbox", { name: "Search memories" }).fill("event-sourced");
    await page.getByRole("button", { name: /Orders are event-sourced/ }).first().click();
    await page.getByText("Recorded by").waitFor();
    await page.getByText("Alternatives considered").waitFor();
    await shot("memory-explorer");
  });
  await step("stale memory explains why", async () => {
    await page.getByRole("textbox", { name: "Search memories" }).fill("");
    await page.getByRole("button", { name: "Possibly stale", exact: true }).click();
    await page.getByRole("button", { name: /Stripe client wrapper/ }).first().click();
    await page.getByText("Why this may be outdated").waitFor();
    await shot("memory-stale");
    await page.getByRole("button", { name: "Possibly stale", exact: true }).click();
  });
  await step("create, pin and delete a memory", async () => {
    await page.getByRole("button", { name: "New memory" }).click();
    await page.getByLabel("Title").fill("UI test memory");
    await page.getByLabel("Content").fill("Created by the UI end-to-end test.");
    await page.getByRole("button", { name: "Save memory" }).click();
    await page.getByRole("textbox", { name: "Search memories" }).fill("UI test memory");
    await page.getByRole("button", { name: /UI test memory/ }).first().click();
    await page.getByRole("button", { name: "Pin", exact: true }).click();
    await page.getByText("pinned").first().waitFor();
    await main.getByRole("button", { name: "Delete", exact: true }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Delete", exact: true }).click();
    await page.getByText("No memories match").waitFor();
  });
  await step("session timeline lists all three agents", async () => {
    await nav("Timeline");
    await heading("Session timeline");
    for (const a of ["Claude Code", "Codex", "Antigravity CLI"]) await main.locator("li button[aria-expanded]", { hasText: a }).first().waitFor();
    await page.locator("li button[aria-expanded]").first().click();
    await page.getByText("Files touched").waitFor();
    await shot("timeline");
  });
  await step("handoff center generates a brief", async () => {
    await nav("Handoffs");
    await heading("Handoff center");
    await page.getByRole("button", { name: "Generate handoff" }).click();
    await page.getByText("# Handoff: acme-shop").waitFor();
    await shot("handoffs");
  });
  await step("integration manager shows detection and state", async () => {
    await nav("Integrations");
    await heading("Integrations");
    await page.getByText("MCP server").first().waitFor({ timeout: 30000 });
    await page.getByText("OpenAI Codex").first().waitFor();
    await shot("integrations");
  });
  await step("integration install plan previews diffs without writing", async () => {
    const card = page.locator("section", { hasText: "Google Antigravity" });
    const btn = card.getByRole("button", { name: /^(Install|Repair)$/ }).first();
    await btn.click();
    await page.getByRole("dialog").getByText("Review every change").waitFor();
    await shot("integration-plan");
    await page.getByRole("dialog").getByRole("button", { name: "Cancel" }).click();
  });
  await step("settings persist", async () => {
    await nav("Settings");
    await heading("Settings");
    const budget = page.getByLabel("Startup brief budget (tokens)");
    const next = String(Number(await budget.inputValue()) === 1400 ? 1300 : 1400);
    await budget.fill(next);
    await page.getByRole("button", { name: "Save changes" }).click();
    await page.getByText("Settings saved").waitFor();
    await nav("Overview");
    await nav("Settings");
    if ((await page.getByLabel("Startup brief budget (tokens)").inputValue()) !== next) throw new Error("budget not persisted");
    await shot("settings");
  });
  await step("command palette searches memories (Ctrl+K)", async () => {
    await page.keyboard.press("Control+k");
    await page.getByRole("dialog", { name: "Command palette" }).getByRole("textbox").fill("refund");
    await page.getByRole("dialog", { name: "Command palette" }).getByRole("option").first().waitFor();
    await page.keyboard.press("Enter");
    await heading("Memory");
  });
  await step("light theme renders", async () => {
    await nav("Settings");
    await page.getByLabel("Theme").selectOption("light");
    await page.getByRole("button", { name: "Save changes" }).click();
    await page.waitForFunction(() => document.documentElement.dataset.theme === "light");
    await nav("Overview");
    await shot("overview-light");
    await nav("Settings");
    await page.getByLabel("Theme").selectOption("dark");
    await page.getByRole("button", { name: "Save changes" }).click();
  });
  await step("no console errors", async () => {
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await nav("Overview");
    await page.waitForTimeout(1500);
    if (errors.length) throw new Error(errors.join("; "));
  });
} catch (e) {
  results.push({ name: "harness", ok: false, error: String(e?.message ?? e) });
  console.error(e);
} finally {
  await browser?.close().catch(() => {});
  app.kill();
}
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} UI checks passed`);
process.exit(failed.length ? 1 : 0);
