import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";

const failures = new Map<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  failures.set(page, errors);
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (entry) => {
    if (
      entry.type() === "error" &&
      /content security policy|refused to/i.test(entry.text())
    )
      errors.push(entry.text());
  });
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill("ui-test-password-only");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.getByRole("tab", { name: "Movies", exact: true }).click();
  await expect(page.getByTestId("virtual-grid")).toHaveAttribute(
    "data-total",
    "5200",
  );
});
test.afterEach(async ({ page }) => {
  expect(failures.get(page)).toEqual([]);
});

test("5,202 titles use bounded DOM, immediate search and genuine show hierarchy", async ({
  page,
}) => {
  const cards = page.locator(".title-card");
  expect(await cards.count()).toBeLessThan(100);
  await page.evaluate(() =>
    window.scrollTo(0, document.documentElement.scrollHeight * 0.65),
  );
  await expect(cards.first()).toBeVisible();
  expect(await cards.count()).toBeLessThan(100);
  await page.getByLabel("Search your library").fill("Library Film 5199");
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toContainText("Library Film 5199");
  await page.getByLabel("Clear search").click();
  await page.getByRole("tab", { name: "Series", exact: true }).click();
  await expect(cards).toHaveCount(2);
  await expect(cards.first()).toContainText("2 seasons");
  await page
    .getByRole("button", { name: "Details for Northstar", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Northstar", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".episode-row")).toHaveCount(3);
  await page.getByRole("tab", { name: "Season 2", exact: true }).click();
  await expect(page.locator(".episode-row").first()).toContainText("S02E01");
  await page.screenshot({
    path: "../validation/spa/show-desktop.png",
    fullPage: true,
  });
  await page
    .getByRole("button", { name: "Back to library", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: /^Your series/ }),
  ).toBeVisible();
});

test("movie detail sheet shows accurate genre, quality and stream metadata", async ({
  page,
}) => {
  await page.getByLabel("Search your library").fill("Afterlight");
  await page
    .getByRole("button", { name: "Details for Afterlight", exact: true })
    .click();
  const sheet = page.getByRole("dialog");
  await expect(
    sheet.getByRole("heading", { name: "Afterlight", exact: true }),
  ).toBeVisible();
  await expect(
    sheet.getByText("Science Fiction", { exact: true }),
  ).toBeVisible();
  await expect(sheet.getByText("4K UHD", { exact: true })).toBeVisible();
  await expect(sheet.getByText("HDR10", { exact: true })).toBeVisible();
  await expect(sheet.locator(".spec-grid")).toContainText("AAC");
  await expect(sheet.locator(".spec-grid")).toContainText("SUBRIP");
  await page.screenshot({
    path: "../validation/spa/movie-sheet.png",
    fullPage: true,
  });
  await page.keyboard.press("Escape");
  await expect(sheet).not.toBeVisible();
});

async function startFilm(page: Page) {
  await page
    .getByRole("button", { name: "Resume Afterlight", exact: true })
    .click();
  const video = page.locator("#persistent-video");
  await expect
    .poll(() =>
      video.evaluate(
        (v: HTMLVideoElement) =>
          !v.paused && v.currentTime > 0 && v.readyState >= 3,
      ),
    )
    .toBe(true);
  return video;
}

test("one video keeps decoding while docking, searching, opening sheets and expanding", async ({
  page,
}) => {
  const video = await startFilm(page);
  await video.evaluate((v: HTMLVideoElement) => {
    (window as any).__originalVideo = v;
    (window as any).__sourceEvents = 0;
    v.addEventListener("emptied", () => (window as any).__sourceEvents++);
    v.addEventListener("loadstart", () => (window as any).__sourceEvents++);
  });
  const src = await video.getAttribute("src");
  const before = await video.evaluate((v: HTMLVideoElement) => v.currentTime);
  await page
    .getByRole("button", { name: "Browse while playing", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Mini player", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Search your library").fill("Afterlight");
  await page
    .getByRole("button", { name: "Details for Afterlight", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByLabel("Clear search").click();
  await page.getByRole("tab", { name: "Series", exact: true }).click();
  await page
    .getByRole("button", { name: "Details for Northstar", exact: true })
    .click();
  await expect(page.locator(".episode-row")).toHaveCount(3);
  await page.screenshot({
    path: "../validation/spa/mini-player.png",
    fullPage: false,
  });
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime))
    .toBeGreaterThan(before + 1);
  expect(
    await video.evaluate((v) => v === (window as any).__originalVideo),
  ).toBe(true);
  expect(await page.evaluate(() => (window as any).__sourceEvents)).toBe(0);
  expect(await video.getAttribute("src")).toBe(src);
  expect(await video.evaluate((v: HTMLVideoElement) => v.paused)).toBe(false);
  await page
    .getByRole("button", { name: "Expand player", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Cinema player", exact: true }),
  ).toBeVisible();
  expect(
    await video.evaluate((v) => v === (window as any).__originalVideo),
  ).toBe(true);
  expect(await page.evaluate(() => (window as any).__sourceEvents)).toBe(0);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => v.paused))
    .toBe(true);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page
    .getByRole("button", { name: "Forward 10 seconds", exact: true })
    .click();
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime))
    .toBeGreaterThan(before + 8);
});

test("audio switching uses ordinal mapping and subtitles survive docking", async ({
  page,
}) => {
  const video = await startFilm(page);
  const requests: string[] = [];
  page.on("request", (request) => {
    if (request.url().includes("/stream?")) requests.push(request.url());
  });
  await page
    .getByRole("button", { name: "Playback settings", exact: true })
    .click();
  await page.getByRole("menuitemradio", { name: /Deutsch/ }).click();
  await expect
    .poll(() =>
      requests.some(
        (url) => url.includes("audio_index=1") && url.includes("mode=remux"),
      ),
    )
    .toBe(true);
  await expect
    .poll(() =>
      video.evaluate((v: HTMLVideoElement) => !v.paused && v.readyState >= 3),
    )
    .toBe(true);
  await page
    .getByRole("button", { name: "Playback settings", exact: true })
    .click();
  await page
    .getByRole("menuitemradio", { name: "English", exact: true })
    .click();
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => v.textTracks[0]?.mode))
    .toBe("showing");
  await page
    .getByRole("button", { name: "Browse while playing", exact: true })
    .click();
  expect(
    await video.evaluate((v: HTMLVideoElement) => v.textTracks[0]?.mode),
  ).toBe("showing");
  expect(await video.evaluate((v: HTMLVideoElement) => v.paused)).toBe(false);
});

test("profile changes isolate continue watching and new profiles persist", async ({
  page,
}) => {
  await page.getByRole("tab", { name: "Home", exact: true }).click();
  await page.getByRole("button", { name: /^Profile:/ }).click();
  await page
    .getByRole("menuitemradio", { name: "Guests", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Resume The Arrival", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Resume Afterlight", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Resume The Arrival", exact: true })
    .click();
  await expect
    .poll(() =>
      page
        .locator("#persistent-video")
        .evaluate((v: HTMLVideoElement) => !v.paused && v.currentTime > 0),
    )
    .toBe(true);
  await page
    .getByRole("button", { name: "Browse while playing", exact: true })
    .click();
  await page.getByRole("button", { name: /^Profile:/ }).click();
  await page.getByRole("menuitemradio", { name: "admin", exact: true }).click();
  await expect(page.locator(".player.is-active")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Resume Afterlight", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: /^Profile:/ }).click();
  await page
    .getByRole("menuitem", { name: "Add profile", exact: true })
    .click();
  await page.getByLabel("Profile name", { exact: true }).fill("Browser Guest");
  await page
    .getByRole("button", { name: "Create profile", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Profile: Browser Guest", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".resume-card")).toHaveCount(0);
  await page.reload();
  await expect(
    page.getByRole("button", { name: "Profile: Browser Guest", exact: true }),
  ).toBeVisible();
});

test("mobile catalog, dialog and mini player stay inside the viewport", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    page.getByRole("tab", { name: "Movies", exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: "../validation/spa/library-mobile.png",
    fullPage: false,
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await startFilm(page);
  await page
    .getByRole("button", { name: "Browse while playing", exact: true })
    .click();
  const mini = page.getByRole("region", { name: "Mini player", exact: true });
  const box = await mini.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  await expect(
    mini.getByRole("slider", { name: "Playback position", exact: true }),
  ).toBeVisible();
  await expect(
    mini.getByRole("slider", { name: "Volume", exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: "../validation/spa/mini-mobile.png",
    fullPage: false,
  });
});

test("desktop hero and scroll budget remain bounded for the full catalog", async ({
  page,
}) => {
  await page.screenshot({
    path: "../validation/spa/library-desktop.png",
    fullPage: false,
  });
  await page.evaluate(() => window.scrollTo(0, 650));
  await page.screenshot({
    path: "../validation/spa/library-grid.png",
    fullPage: false,
  });
  const measurements = await page.evaluate(async () => {
    const frames: number[] = [];
    let previous = performance.now();
    let maxCards = 0;
    for (let n = 0; n < 90; n++) {
      await new Promise(requestAnimationFrame);
      const now = performance.now();
      frames.push(now - previous);
      previous = now;
      window.scrollTo(0, 800 + n * 110);
      maxCards = Math.max(
        maxCards,
        document.querySelectorAll(".title-card").length,
      );
    }
    frames.sort((a, b) => a - b);
    return {
      frames: frames.length,
      median_ms: frames[Math.floor(frames.length / 2)],
      p95_ms: frames[Math.floor(frames.length * 0.95)],
      max_mounted_cards: maxCards,
    };
  });
  expect(measurements.max_mounted_cards).toBeLessThan(100);
  console.log("Scroll fixture measurements:", JSON.stringify(measurements));
  await test.info().attach("scroll-budget.json", {
    body: JSON.stringify(measurements, null, 2),
    contentType: "application/json",
  });
});

test("format failures recover through remux and conversion without losing position or looping", async ({
  page,
}) => {
  const video = await startFilm(page);
  await video.evaluate((v: HTMLVideoElement) => {
    Object.defineProperty(v, "error", {
      configurable: true,
      value: { code: 2 },
    });
    v.dispatchEvent(new Event("error"));
  });
  await expect(video).toHaveAttribute("src", /mode=direct/);
  const position = await video.evaluate((v: HTMLVideoElement) => v.currentTime);
  const remux = page.waitForRequest((r) => r.url().includes("mode=remux"));
  await video.evaluate((v: HTMLVideoElement) => {
    Object.defineProperty(v, "error", {
      configurable: true,
      value: { code: 4 },
    });
    v.dispatchEvent(new Event("error"));
    delete (v as any).error;
  });
  expect(
    Number(new URL((await remux).url()).searchParams.get("start_seconds")),
  ).toBeGreaterThanOrEqual(position - 1);
  await expect
    .poll(() =>
      video.evaluate(
        (v: HTMLVideoElement, position: number) =>
          !v.paused && v.readyState >= 3 && v.currentTime >= position,
        position,
      ),
    )
    .toBe(true);
  const converted = page.waitForRequest((r) =>
    r.url().includes("mode=transcode"),
  );
  await video.evaluate((v: HTMLVideoElement) => {
    Object.defineProperty(v, "error", {
      configurable: true,
      value: { code: 3 },
    });
    v.dispatchEvent(new Event("error"));
    delete (v as any).error;
  });
  await converted;
  await expect
    .poll(() =>
      video.evaluate(
        (v: HTMLVideoElement, position: number) =>
          !v.paused && v.readyState >= 3 && v.currentTime >= position,
        position,
      ),
    )
    .toBe(true);
  const finalSource = await video.getAttribute("src");
  await video.evaluate((v: HTMLVideoElement) => {
    Object.defineProperty(v, "error", {
      configurable: true,
      value: { code: 3 },
    });
    v.dispatchEvent(new Event("error"));
    delete (v as any).error;
  });
  await expect(page.getByRole("alert")).toContainText(
    "This format could not play",
  );
  await expect(video).toHaveAttribute("src", finalSource!);
});

test("preferred audio uses a selectable stream instead of silently playing default audio", async ({
  page,
}) => {
  await page.route("**/api/v1/playback-defaults", (route) =>
    route.fulfill({
      json: { audio_language_preference: "deu", subtitle_mode: "off" },
    }),
  );
  const remux = page.waitForRequest(
    (r) => r.url().includes("mode=remux") && r.url().includes("audio_index=1"),
  );
  await startFilm(page);
  await remux;
});

test("an unsupported original response recovers through actual remux playback", async ({
  page,
}) => {
  await page.route("**/api/v1/items/*/stream?**", (route) => {
    if (new URL(route.request().url()).searchParams.get("mode") === "direct")
      return route.fulfill({
        contentType: "video/x-unsupported",
        body: "unsupported media fixture",
      });
    return route.continue();
  });
  const remux = page.waitForRequest((r) => r.url().includes("mode=remux"));
  await startFilm(page);
  await remux;
});

test("Anime includes films and shows, search and episode navigation", async ({
  page,
}) => {
  await page.getByRole("tab", { name: "Anime", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Your anime" })).toBeVisible();
  await expect(page.locator(".title-card")).toHaveCount(2);
  await expect(
    page.getByRole("button", {
      name: "Details for A Quiet Orbit",
      exact: true,
    }),
  ).toBeVisible();
  await page.getByLabel("Search your library").fill("Northstar");
  await expect(page.locator(".title-card")).toHaveCount(1);
  await page
    .getByRole("button", { name: "Details for Northstar", exact: true })
    .click();
  await expect(page.locator(".episode-row")).toHaveCount(3);
  await expect(page.locator(".episode-row").nth(1)).toContainText("S01E2-3");
  await expect(page.locator(".episode-row").nth(2)).toContainText("S01E3.5");
  await page
    .getByRole("button", { name: "Back to library", exact: true })
    .click();
  await page.getByLabel("Clear search").click();
  await page.screenshot({ path: "../validation/anime-desktop.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    page.getByRole("tab", { name: "Anime", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({ path: "../validation/anime-mobile.png" });
});
