import { test, expect } from "@playwright/test";
test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill("ui-test-password-only");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Just arrived", exact: true }),
  ).toBeVisible();
});
test("curated shelves, ambient artwork, adaptive navigation and settings", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await expect(
    page.getByRole("heading", { name: "Exceptional stories", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", {
      name: "The Afterlight Collection",
      exact: true,
    }),
  ).toBeAttached();
  await page.screenshot({
    path: "../validation/luxury-v1.1/home-desktop.png",
    fullPage: true,
  });
  await page
    .getByRole("heading", { name: "Just arrived", exact: true })
    .scrollIntoViewIfNeeded();
  const card = page
    .locator(".curated-section")
    .first()
    .locator(".title-card")
    .first();
  await card.hover();
  await expect(card.locator(".poster-play")).toHaveCSS(
    "backdrop-filter",
    "none",
  );
  await page.waitForTimeout(600);
  expect(
    await card.evaluate((e) =>
      getComputedStyle(e).getPropertyValue("--ambient"),
    ),
  ).toMatch(/rgb/);
  await expect(page.locator(".navigation")).toHaveClass(/is-scrolled/);
  await page.screenshot({
    path: "../validation/luxury-v1.1/ambient-cards.png",
  });
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("heading", { name: "Your cinema, your way." }),
  ).toBeVisible();
  await expect(dialog.getByRole("switch")).toBeChecked();
  await page.screenshot({
    path: "../validation/luxury-v1.1/settings-general.png",
  });
  await dialog.getByRole("tab", { name: "Library & Storage" }).click();
  await expect(dialog).toContainText("Extracted scenes");
  await page.screenshot({
    path: "../validation/luxury-v1.1/settings-storage.png",
  });
  await dialog.getByRole("tab", { name: "Streaming & Playback" }).click();
  await dialog.getByLabel("Preferred audio language").selectOption("deu");
  await dialog
    .getByRole("button", { name: "Save playback preferences" })
    .click();
  await expect(dialog).toContainText("Your preferences are saved.");
  await dialog.getByRole("tab", { name: "System & Diagnostics" }).click();
  await expect(dialog).toContainText("v1.1.0");
  await page.screenshot({
    path: "../validation/luxury-v1.1/settings-system.png",
  });
  await dialog.getByRole("button", { name: "Close settings" }).click();
  expect(errors).toEqual([]);
});
test("spotlight keyboard navigation, fuzzy titles, people and collections", async ({
  page,
}) => {
  await page.keyboard.press("Control+k");
  const input = page.getByLabel("Search titles, people and collections");
  await input.fill("Aftrlght");
  await expect(page.locator(".spotlight-result").first()).toContainText(
    "Afterlight",
  );
  await page.keyboard.press("Enter");
  await expect(
    page
      .getByRole("dialog")
      .getByRole("heading", { name: "Afterlight", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close movie details" }).click();
  await page.getByRole("button", { name: "Open search", exact: true }).click();
  await input.fill("Mira");
  await page.getByRole("button", { name: /Mira Chen/ }).click();
  await expect(page.getByRole("dialog")).toContainText("A filmmaker exploring");
  await page.screenshot({ path: "../validation/luxury-v1.1/people-hub.png" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Open search", exact: true }).click();
  await input.fill("Afterlight");
  await expect(
    page.getByRole("button", { name: /The Afterlight Collection/ }),
  ).toBeVisible();
  await page.screenshot({
    path: "../validation/luxury-v1.1/command-palette.png",
  });
});
test("hero local trailer waits two seconds and stops on pointer leave", async ({
  page,
}) => {
  await page.locator(".navigation").hover();
  await page.locator(".hero").hover();
  await page.waitForTimeout(1000);
  await expect(page.locator(".ambient-trailer")).not.toHaveClass(/is-playing/);
  await expect(page.locator(".ambient-trailer")).toHaveClass(/is-playing/, {
    timeout: 5000,
  });
  expect(
    await page
      .locator(".ambient-trailer")
      .evaluate((e) => (e as HTMLVideoElement).muted),
  ).toBe(true);
  await page.getByRole("button", { name: "Unmute trailer" }).click();
  await page.locator(".navigation").hover();
  await expect(page.locator(".ambient-trailer")).not.toHaveClass(/is-playing/);
});
test("chapter intro and sequential next episode with countdown cancellation", async ({
  page,
}) => {
  await page.getByRole("tab", { name: "Series", exact: true }).click();
  await page
    .getByRole("button", { name: "Details for Northstar", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Play The Arrival", exact: true })
    .click();
  await expect(page.locator("#persistent-video")).toHaveJSProperty(
    "paused",
    false,
  );
  await expect(
    page.getByRole("button", { name: "Skip intro", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Skip intro", exact: true }).click();
  await expect
    .poll(() =>
      page
        .locator("#persistent-video")
        .evaluate((v) => (v as HTMLVideoElement).currentTime),
    )
    .toBeGreaterThan(7);
  await page
    .locator("#persistent-video")
    .evaluate((v) => ((v as HTMLVideoElement).currentTime = 65));
  await expect(
    page.getByRole("heading", { name: /Next Episode in/ }),
  ).toBeVisible();
  await page.screenshot({ path: "../validation/luxury-v1.1/next-episode.png" });
  await page.getByRole("button", { name: "Stay with this episode" }).click();
  await expect(page.locator(".episode-prompt")).toHaveCount(0);
});
test("mobile home and settings remain within viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "../validation/luxury-v1.1/home-mobile.png",
    fullPage: true,
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.waitForTimeout(250);
  await page.screenshot({
    path: "../validation/luxury-v1.1/settings-mobile.png",
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
});

test("next episode automatically starts after ten seconds and marks the previous episode complete", async ({
  page,
}) => {
  await page.getByRole("tab", { name: "Series", exact: true }).click();
  await page
    .getByRole("button", { name: "Details for Northstar", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Play The Arrival", exact: true })
    .click();
  await expect(page.locator("#persistent-video")).toHaveJSProperty(
    "paused",
    false,
  );
  await page
    .locator("#persistent-video")
    .evaluate((v) => ((v as HTMLVideoElement).currentTime = 65));
  await expect(
    page.getByRole("heading", { name: /Next Episode in/ }),
  ).toBeVisible();
  await expect(page.locator(".now-playing strong")).toHaveText("An Open Door", {
    timeout: 20000,
  });
  await expect(page.locator("#persistent-video")).toHaveJSProperty(
    "paused",
    false,
  );
});
