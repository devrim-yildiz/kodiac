import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
async function login(
  page: Page,
  name = "admin",
  password = "ui-test-password-only",
) {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill(name);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Home", exact: true }),
  ).toBeVisible();
}
const defaults = {
  audio_language_preference: "eng",
  subtitle_mode: "off",
  home_sections: ["continue", "collections", "recommended", "top"],
};
test("home preferences persist, collection opens fully, filters and sorting work", async ({
  page,
}) => {
  await login(page);
  await page.request.put("/api/v1/preferences", { data: defaults });
  await page.reload();
  await expect(page.locator(".hero")).toHaveCount(0);
  await expect(page.locator(".site-footer")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Customize Home" }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await page.getByRole("button", { name: "Home screen", exact: true }).click();
  await page
    .getByRole("checkbox", { name: "Top rated", exact: true })
    .uncheck();
  await page
    .getByRole("button", { name: "Move Collections up", exact: true })
    .click();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByText("Changes saved.", { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.locator("main > .collections-section")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Top rated", exact: true }),
  ).toHaveCount(0);
  const order = await page
    .locator("main > section")
    .evaluateAll((nodes) => nodes.map((n) => n.className));
  expect(order[0]).toContain("collections-section");
  await page.locator(".collection-card").first().click();
  await expect(
    page.getByRole("heading", {
      name: "The Afterlight Collection",
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.getByTestId("virtual-grid")).toHaveAttribute(
    "data-total",
    "2",
  );
  await page.getByRole("tab", { name: "Movies", exact: true }).click();
  await page.getByLabel("Sort by", { exact: true }).selectOption("title-desc");
  await expect(page.locator(".title-card").first()).toContainText(
    "Library Film 5199",
  );
  await page.getByLabel("Genre", { exact: true }).selectOption("878");
  await page.getByLabel("Search your library").fill("Afterlight");
  await expect(page.locator(".title-card")).toHaveCount(1);
  await page.reload();
  await page.getByRole("tab", { name: "Movies", exact: true }).click();
  await expect(page.getByLabel("Sort by", { exact: true })).toHaveValue(
    "title-desc",
  );
  await page.request.put("/api/v1/preferences", { data: defaults });
});
test("admin creates a regular account whose settings stay private", async ({
  page,
}) => {
  await login(page);
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await page.getByRole("button", { name: "Users", exact: true }).click();
  await page.locator(".create-account > summary").click();
  await page.getByLabel("Username", { exact: true }).fill("browser-viewer");
  await page
    .getByLabel("Initial password", { exact: true })
    .fill("browser-viewer-password");
  await page
    .getByRole("button", { name: "Create account", exact: true })
    .click();
  await expect(
    page.getByText("Account created for browser-viewer.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: /^Profile:/ }).click();
  await page.getByRole("menuitem", { name: "Sign out", exact: true }).click();
  await login(page, "browser-viewer", "browser-viewer-password");
  const serverRequests: string[] = [];
  page.on("request", (request) => {
    if (/\/api\/v1\/(settings|users)/.test(request.url()))
      serverRequests.push(request.url());
  });
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Users", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Libraries", exact: true }),
  ).toHaveCount(0);
  expect((await page.request.get("/api/v1/settings")).status()).toBe(403);
  expect((await page.request.get("/api/v1/users")).status()).toBe(403);
  expect((await page.request.get("/api/v1/settings/stats")).status()).toBe(403);
  expect(serverRequests).toEqual([]);
  await page.getByRole("button", { name: "Playback", exact: true }).click();
  await page.getByLabel("Preferred audio language").selectOption("jpn");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByText("Changes saved.", { exact: true })).toBeVisible();
  expect(
    (await (await page.request.get("/api/v1/playback-defaults")).json())
      .audio_language_preference,
  ).toBe("jpn");
});
test("player hides chrome, keeps keyboard access, centers transport and seeks", async ({
  page,
}) => {
  await login(page);
  await page
    .getByRole("button", { name: "Resume Afterlight", exact: true })
    .click();
  const video = page.locator("#persistent-video");
  await expect
    .poll(() =>
      video.evaluate((v: HTMLVideoElement) => !v.paused && v.readyState >= 3),
    )
    .toBe(true);
  await page.mouse.move(10, 450);
  await expect(page.locator(".player")).toHaveClass(/controls-hidden/, {
    timeout: 6000,
  });
  await page.keyboard.press("Tab");
  await expect(page.locator(".player")).toHaveClass(/controls-visible/);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => v.paused))
    .toBe(true);
  await page
    .getByRole("slider", { name: "Playback position", exact: true })
    .focus();
  await page.keyboard.press("End");
  await page.keyboard.press("ArrowLeft");
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime))
    .toBeGreaterThan(85);
  await page.setViewportSize({ width: 390, height: 844 });
  const buttons = await Promise.all(
    ["Back 10 seconds", "Play", "Forward 10 seconds"].map((name) =>
      page.getByRole("button", { name, exact: true }).boundingBox(),
    ),
  );
  expect(Math.abs(buttons[1]!.x + buttons[1]!.width / 2 - 195)).toBeLessThan(1);
  expect(
    Math.abs(
      buttons[0]!.y +
        buttons[0]!.height / 2 -
        (buttons[2]!.y + buttons[2]!.height / 2),
    ),
  ).toBeLessThan(1);
  const labels = await page.locator(".skip-icon").evaluateAll((nodes) =>
    nodes.map((n) => {
      const icon = n.getBoundingClientRect();
      const label = n.querySelector("span")!.getBoundingClientRect();
      return Math.abs(icon.x + icon.width / 2 - label.x - label.width / 2);
    }),
  );
  expect(labels.every((n) => n < 1)).toBe(true);
});

test("touch player menus stay in the viewport and settings expose every category", async ({
  browser,
}) => {
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  const page = await context.newPage();
  await login(page);
  await page.getByRole("button", { name: "Open settings", exact: true }).tap();
  await page.getByRole("button", { name: "Open metadata", exact: true }).tap();
  await expect(
    page.getByRole("heading", { name: "Metadata", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Settings", exact: true }).tap();
  await page.getByRole("button", { name: "Open playback", exact: true }).tap();
  await expect(page.getByLabel("Preferred audio language")).toBeVisible();
  await page.getByRole("tab", { name: "Movies", exact: true }).tap();
  await page.getByLabel("Search your library").fill("Afterlight");
  await page
    .getByRole("button", { name: "Details for Afterlight", exact: true })
    .tap();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: /^(Play|Resume)$/ })
    .tap();
  await expect
    .poll(() =>
      page
        .locator("#persistent-video")
        .evaluate((v: HTMLVideoElement) => !v.paused && v.readyState >= 3),
    )
    .toBe(true);
  await expect(page.locator(".player")).toHaveClass(/controls-hidden/, {
    timeout: 6000,
  });
  await page.locator("#persistent-video").tap();
  await expect(
    page.getByRole("button", { name: "Pause", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Playback settings", exact: true })
    .tap();
  await page
    .getByRole("menuitem", { name: "Audio & subtitles", exact: true })
    .tap();
  const menu = await page.getByRole("menu").boundingBox();
  expect(menu!.x).toBeGreaterThanOrEqual(0);
  expect(menu!.x + menu!.width).toBeLessThanOrEqual(390);
  await page.getByRole("menuitemradio", { name: "English", exact: true }).tap();
  await expect
    .poll(() =>
      page
        .locator("#persistent-video")
        .evaluate((v: HTMLVideoElement) => v.textTracks[0]?.mode),
    )
    .toBe("showing");
  await context.close();
});

test("server overview shows measured stats, refresh errors and recovers", async ({
  page,
}) => {
  await login(page);
  let fail = false;
  await page.route("**/api/v1/settings/stats", async (route) => {
    if (fail)
      await route.fulfill({
        status: 503,
        json: { error: "Temporary test outage" },
      });
    else
      await route.fulfill({
        json: {
          version: "1.1.0",
          architecture: "aarch64",
          ffmpeg: "FFmpeg fixture",
          uptime_seconds: 90120,
          db_bytes: 2097152,
          artwork_bytes: null,
          media_count: 5202,
          metadata_pending: 7,
          scan: { running: false, indexed: 5202, phase: "idle" },
          active_streams: [{ title: "Afterlight", profile: "Guests" }],
          workers: [
            { name: "scanner", status: "paused", updated_at: 1 },
            { name: "metadata", status: "running", updated_at: 1 },
          ],
          logs: [],
        },
      });
  });
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Server overview", exact: true })
    .click();
  await expect(page.locator(".server-facts")).toContainText("1d 1h 2m");
  await expect(page.locator(".server-facts")).toContainText("5,202");
  await expect(page.locator(".server-facts")).toContainText("Unavailable");
  await expect(page.locator(".worker-list")).toContainText("Not responding");
  await expect(page.locator(".worker-list")).toContainText("paused");
  await expect(page.locator(".stream-row")).toContainText("Afterlight");
  fail = true;
  await page
    .getByRole("button", { name: "Refresh server stats", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText("Temporary test outage");
  fail = false;
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("mobile Settings drills into grouped pages and guards unsaved edits", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await login(page);
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Settings", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("combobox", { name: "Settings section" }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Open playback", exact: true })
    .click();
  await page.getByLabel("Preferred audio language").selectOption("fra");
  page.once("dialog", (dialog) => dialog.dismiss());
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByLabel("Preferred audio language")).toHaveValue("fra");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByText("Changes saved.", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Open playback", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.request.put("/api/v1/preferences", { data: defaults });
});
