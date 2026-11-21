# 0003. Web UI: React + Vite + TypeScript, served by the Rust binary

- Status: Accepted
- Date: 2026-11-21

## Context

The UI must run in any browser against a headless Raspberry Pi. Options were React or Vue/Vuetify.

## Decision

React 18 + TypeScript, built with Vite into static files (`web/dist`) that the Rust server
serves. No component library, no state-management library: the whole UI is ~600 lines, state
is the server's `Status` pushed over a WebSocket, and actions are plain `fetch` calls.

## Why not Vuetify

Vuetify gives ready-made widgets, but this UI is a handful of sliders, buttons and a live image
whose important parts (the tracker overlay, hold-to-slew pad, click-to-pick feature) are custom
anyway. Fewer dependencies means a smaller bundle (~50 kB gzipped) on a Pi over Wi-Fi, and no
framework upgrade treadmill. A component library can be adopted later without touching the API.

## Consequences

Node is needed only to *build* the UI; the Pi runs one binary plus static files. The UI is
mobile-friendly enough to use at the telescope on a phone; styling is ours to maintain.
