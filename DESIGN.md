---
name: Kodiac
description: A personal screening library with precise controls.
colors:
  bg: "#101012"
  surface: "#1c1c1f"
  raised: "#29292d"
  text: "#f5f5f7"
  secondary: "#b1b1b8"
  line: "#ffffff18"
  accent: "#a8caff"
typography:
  headline:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Display", "Segoe UI", sans-serif'
    fontSize: "clamp(30px, 3vw, 40px)"
    fontWeight: 700
    lineHeight: 1.15
    letterSpacing: "-0.025em"
  title:
    fontSize: "25px"
    fontWeight: 650
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  body:
    fontSize: "15px"
    lineHeight: 1.6
  label:
    fontSize: "14px"
    fontWeight: 600
rounded:
  field: "9px"
  control: "10px"
  card: "12px"
  dialog: "16px"
spacing:
  tight: "8px"
  compact: "12px"
  regular: "20px"
  roomy: "24px"
components:
  button-primary:
    backgroundColor: "{colors.text}"
    textColor: "#161618"
    rounded: "{rounded.control}"
    padding: "11px 20px"
    typography: "{typography.label}"
  button-secondary:
    backgroundColor: "#ffffff12"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "11px 20px"
    typography: "{typography.label}"
  button-icon:
    rounded: "{rounded.control}"
    width: "44px"
    height: "44px"
  input:
    backgroundColor: "#141416"
    textColor: "{colors.text}"
    rounded: "{rounded.field}"
    padding: "10px 12px"
  navigation:
    textColor: "{colors.secondary}"
    padding: "10px 22px"
    rounded: "{rounded.field}"
  season-chip:
    backgroundColor: "#ffffff08"
    textColor: "{colors.secondary}"
    rounded: "8px"
    padding: "12px 20px"
  poster-card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.card}"
---

# Design System: Kodiac

## Overview

**Creative North Star: "A personal screening library"**

Apple TV is the confirmed visual reference: restrained dark neutrals, system sans typography and white primary actions. Media supplies the imagery; the interface supplies stable organization, clear labels and precise playback controls. Modern Jellyfin utility informs the density and completeness of administration.

Depth comes from tonal surfaces and purposeful overlays. Motion responds to opening details, selecting media and revealing playback controls. Desktop and mobile browsers share the system; native TV clients are separate products.

**Key Characteristics:**
- Media-first shelves and searchable poster grids.
- Quiet neutral surfaces with white primary actions.
- Readable utility controls and visible keyboard focus.
- Purposeful motion with reduced-motion support.

## Colors

The frontmatter records the reused CSS custom properties from `web/src/style.css`; these values are normative.

### Primary
- **Soft white (`text`)** supplies primary actions as well as high-emphasis text.
- **Focus blue (`accent`)** identifies keyboard focus, text carets and native checkbox selection.

### Neutral
- **Cinema black (`bg`)** is the page canvas.
- **Charcoal (`surface`)** supports artwork and dialogs.
- **Raised charcoal (`raised`)** names the raised neutral; dropdowns use its near-opaque equivalent.
- **Muted silver (`secondary`)** carries metadata, descriptions and inactive navigation.
- **Quiet white (`line`)** divides navigation, settings rows and technical details.

**The Focus Color Rule.** Keep blue tied to interaction and focus; primary actions use white.

## Typography

The system sans stack in the frontmatter serves every role, including titles. The headline and title roles distinguish hierarchy through size, weight and tight tracking; labels stay compact and readable.

- Page headings use the headline role; at mobile width they settle at (30px).
- Section headings use the title role, reduced to (22px) on mobile. Settings detail headings use (28px) on desktop and mobile; its page title uses (34px), reduced to (30px) on mobile.
- Body copy uses the body role; the root size becomes (14px) on mobile. Descriptive text is usually muted; synopsis copy extends to (70ch), settings introductions to (65ch).
- Poster titles use (14px), weight (550), line-height (1.4), and a two-line clamp; mobile titles use (13px). Supporting metadata uses (12px).
- Series detail titles use `clamp(32px, 4vw, 56px)`, becoming (36px) on mobile. Movie dialog titles use (34px), becoming (30px).
- Playback times and episode ordinals use tabular numerals.

## Layout

Content is centered with maximum width (1600px). Side gutters step from (48px) to (32px) at (1000px), then (20px) at (640px). Desktop navigation is fixed at the top with height (76px); mobile uses a (64px) top bar and a bottom destination bar of (62px) plus the safe-area inset.

Shelves scroll horizontally with proximity snapping and gaps of (20px), reduced to (14px) on mobile. Posters keep a (2:3) aspect ratio; resume and collection artwork keep (16:9). Shelf posters span `clamp(150px, 13vw, 200px)`, settling at (136px) on mobile. Resume and collection cards span `clamp(260px, 29vw, 380px)`, then (80vw).

Library search and filters sit together on desktop; mobile search fills its own row above two flexible selects. The virtualized library retains a (22px) grid gap and at least two columns. Its column count follows available container width, with denser sizing below (600px) container width.

Settings cap the page width at (1120px), with a (200px) sidebar, a flexible remaining content track and a (48px) gap. At (1000px) the sidebar becomes (170px) and the gap (32px). At (640px), grouped category rows fill the width; choosing a destination opens its detail view with an explicit Settings back link. The sidebar and duplicate page title are hidden in mobile detail views. Server facts use two columns on desktop and one on mobile. The save footer appears only for unsaved edits and sticks above mobile navigation at (62px) plus the safe-area inset.

Movie details center in an (820px) maximum-width dialog on desktop; mobile uses a full-width bottom sheet capped at (94dvh), with safe-area padding. Technical specifications collapse from three columns to one. Player transport uses a symmetric three-track desktop grid and centers over the viewport on mobile.

## Elevation & Depth

Tonal surfaces lead; shadows establish temporary elevation. Menus use `0 12px 40px #0007`, dialogs `0 24px 80px #0008`, toast feedback `0 8px 28px #0006`, and the docked player `0 10px 32px #0006`. Poster hover adds `0 10px 25px #0005` with a small lift; mobile removes that lift and shadow.

Navigation and the shared glass surface use backdrop blur (24px). Video chrome uses black-to-transparent gradients to protect media legibility. Reduced-transparency preferences remove navigation, destination-bar and shared glass blur in favor of an opaque surface; increased-contrast preferences strengthen muted text and separators.

## Shapes

Controls, fields, cards and dialogs use the distinct radius roles in the frontmatter. Season selectors use (8px); quality badges use (4px). Avatars and playback toggles are circular. Artwork is clipped within its rounded frame, without a surrounding decorative card panel. Mobile movie sheets retain rounded top corners and square bottom corners. Settings groups reuse the card radius for neutral row containers with clipped separators.

## Components

### Buttons

Primary and secondary buttons share a minimum height (44px), compact label weight and a (9px) icon gap. Primary hover becomes pure white; secondary and glass button hover strengthens the translucent white fill. Pressing scales these buttons to (0.98). Icon buttons use a (44px) square target and a subtle hover fill. Disabled buttons use opacity (0.42). Keyboard focus uses a blue outline (3px) with offset (4px).

### Chips

Season selectors are horizontally scrollable, muted and translucent at rest. Active selectors strengthen both text and fill. Technical quality badges are small outlined labels, separate from interactive season selectors.

### Cards / Containers

Poster cards pair clipped artwork with a two-line title and muted metadata beneath. Hover lifts artwork by (4px) over (220ms); press scales it to (0.98). Continue Watching uses landscape artwork, a remaining-time label and a white progress line. Collections combine available media artwork. Missing artwork has a neutral text fallback; synthetic review posters and video color bars are test fixtures, never a design asset vocabulary.

### Inputs / Fields

Fields use dark inset backgrounds, a translucent white border and minimum height (44px). Search wraps its icon and field within one outlined group; focus-within uses a blue outline (2px) at offset (2px). Explicit labels remain visible. Error text uses a warm pale red, and disabled controls retain the global disabled treatment.

### Navigation

Desktop destinations sit in a centered segmented row; active destinations gain a translucent fill and high-emphasis text. Mobile moves the same destinations to the safe-area-aware bottom bar. Profile and search remain in the top bar. Settings navigation pairs the desktop sidebar with an overview of account identity and grouped destinations, each with a short summary and chevron. Mobile uses that overview for category drill-down and explicit back navigation. Personal and Server administration remain separate; only administrators receive Server destinations.

### Settings groups

Grouped destination rows use the surface neutral, thin separators, minimum height (62px) and padding (18px 20px), reduced to (17px 16px) on mobile. Hover adds a quiet translucent fill; the focus outline sits inside clipped groups. Account identity remains compact above the groups.

Form groups place labels beside native controls in rows with minimum height (76px); mobile stacks labels and controls. Supporting hints span the row. Unsaved changes reveal a sticky save action and status; an untouched form has no save footer. Account creation expands inline through a disclosure.

Server overview uses quiet label/value rows with tabular values for uptime, media files, streams, metadata queue, database and artwork cache, followed by worker states and current streams. Missing measurements remain explicit. Diagnostics and server events have their own destination.

### Movie details and playback

Movie details present title, metadata, genres, playback action and synopsis in that order. Opening and closing use opacity plus scale (0.96–1), duration (350ms), and `cubic-bezier(0.16, 1, 0.3, 1)`; an interrupted transition resumes from its current presentation. Reduced motion keeps scale fixed and uses a (150ms) fade.

Expanded playback chrome hides after (2800ms) of uninterrupted viewing and fades over (220ms). Input reveals controls; pause, loading, error, keyboard focus, scrubbing and open menus keep them visible. Audio and subtitles live in a submenu. The docked player sits above mobile destinations and preserves the media title and essential playback actions. CSS transitions and animations are shortened under reduced-motion preferences.

**The Viewing Rule.** Controls recede only while uninterrupted playback can continue; interaction and recovery states remain legible.

## Do's and Don'ts

### Do:
- **Do** let library artwork lead and reuse the neutral text fallback when it is unavailable.
- **Do** preserve visible focus, labeled fields, safe-area spacing and reduced-motion behavior.
- **Do** keep personal preferences visibly separate from administrator controls.
- **Do** retain useful media detail backdrops and readable overlays in their existing contexts.

### Don't:
- **Don't** add promotional filler, random rotating billboards or decorative footer copy.
- **Don't** replace white primary actions with a decorative accent color.
- **Don't** turn synthetic review posters or video color bars into product artwork or palette tokens.

<!-- Recorded by Codex on 2026-10-27 from the shipped frontend source. Sources: web/src/style.css, App.tsx, components/Library.tsx, Discovery.tsx, Player.tsx, Settings.tsx, MovieSheet.tsx, ShowPage.tsx, shared.tsx and useSpatialPresence.ts. PRODUCT.md and the approved surface contract supply the confirmed visual authority. Settings refinement refreshed by Codex on 2026-10-27 from Settings.tsx, its stylesheet rules and its scoped surface contract. This source documentation does not substitute for the independent finish review. -->
