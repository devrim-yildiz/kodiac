---
version: 1
slug: "web-src-components-settings-tsx"
primary_target: "web/src/components/Settings.tsx"
related_targets: []
---

# Settings refinement

Mode: Operate. Existing Apple TV visual world and code-first workflow remain binding. User approved grouped personal preferences, separate server administration, mobile category drill-down and removal of the permanent Customize Home action. User also requested useful server statistics.

## Direction contract

THESIS: Settings starts with understandable choices and short current-value summaries; configuration is not the viewing homepage's permanent call to action.

OWN-WORLD: Inherit DESIGN.md dark neutrals, system sans, white actions, restrained separators and 12px grouped surfaces. Groups contain settings rows, not a grid of decorative cards.

STORY: Choose personal playback or Home preferences; administrators can inspect server activity, manage media/accounts, or reach detailed diagnostics. Mobile opens a category, then returns to the same Settings overview.

FIRST VIEWPORT: Desktop has a compact sidebar beside account identity and grouped destinations. Mobile shows the same grouped destinations full-width, replacing the previous category select. A subpage shows a Settings back link, one content heading and compact form rows. Server overview puts uptime, media count, active streams, queue and storage measurements in a quiet definition list, followed by worker states and active sessions.

FORM: Scoped refinement inside the user-pinned Apple TV world; no new identity or concept roll. Signature interaction is category drill-down with explicit back navigation; unsaved changes remain guarded. No new monitoring dependency or fabricated host metrics.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
