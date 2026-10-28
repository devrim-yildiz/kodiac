# Kodiac

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

People watching their own media on desktop and mobile browsers. Administrators also configure and maintain the server here. TV clients will be separate native apps; a native mobile client may follow later.

## Product Purpose

Make a personal movie and series library easy to browse, search and watch, with dependable playback and understandable administration.

## Capabilities and Constraints

Preserve existing media, profiles, progress, playback recovery and Jellyfin compatibility. Keep the initial administrator bootstrap on first deployment; administrators can create administrator and regular accounts. Regular accounts receive personal settings, never server controls or server data. Enforce permissions on the server.

Provide searchable, filterable and sortable libraries, collections, Continue Watching and a configurable home with deliberate ordering. Recently Added is optional and never a navigation destination. Missing collection identification must be investigated rather than hidden by design.

Player controls hide during viewing, remain accessible by touch and keyboard, use accurately centered transport controls, and expose audio and subtitles through an uncluttered submenu.

## Brand Commitments

Apple TV is the binding visual reference; a modern Jellyfin is the functional reference. Remove promotional filler, decorative bottom text and vague labels. The user approved building directly in the frontend after agreeing the layout. Existing English interface language is retained pending a different language request.

## Product Principles

- Media leads; controls explain themselves.
- Stable organization beats random shelves.
- Separate watching preferences from server administration.
- Build useful complete workflows, without a plugin framework or speculative features.

## Evidence on Hand

Existing React/TypeScript frontend, Rust API, synthetic browser fixtures and HTTP tests. No production deployment or private catalog data is part of this rework.

> Recorded by Codex on 2026-10-27 from Devrim's confirmed rework brief; language retention is an explicit implementation assumption.
