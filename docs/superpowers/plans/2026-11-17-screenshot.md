# Screenshot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `shot` tool: frozen overlay, rectangle/window/full-screen selection, copy or save PNG.

**Architecture:** Reuses the picker's capture + per-display overlay pattern. Platform crate adds window listing, portal capture and an `arboard` image clipboard; the app adds `shot.rs` (overlay UI) and `shot_output.rs` (encode/save/copy).

**Tech Stack:** Rust 2024, GPUI (gpui-kit 0.9.1), windows-sys, x11rb, ashpd, arboard, chrono, image (png).

Spec: `docs/superpowers/specs/2026-11-17-screenshot-design.md`

---

### Task 1: Core
- [ ] Tests: `"screenshot".parse() == ToolId::Shot`; `ShotSettings` defaults (`Super+Shift+S`, empty `save_dir`), no default conflicts; `shot_file_name` adds `_2` on clash.
- [ ] Implement `ToolId::Shot`, `ShotSettings`, settings arms, `shot_file_name`.
- [ ] `cargo test -p yaltri-core`; commit `feat(core): add the screenshot tool id and settings`.

### Task 2: Platform
- [ ] `clipboard.rs`: `ImageClipboard::new()`, `set_png_bgra(width, height, bgra)` via arboard (`ImageData` RGBA).
- [ ] `windows_list.rs`: `WinRect { x, y, width, height }`, `visible_windows()`; Windows (EnumWindows + DWM bounds + cloak/minimised/own/shell filters), X11 (`_NET_CLIENT_LIST_STACKING` reversed + frame extents), else empty.
- [ ] `screen.rs`: `capture_portal(interactive) -> Result<Option<Screenshot>>` (ashpd Screenshot, read PNG → BGRA, delete file).
- [ ] Tests for pure helpers (rect clipping, rgba/bgra swap); Windows clippy + Linux cross-check; commit `feat(platform): window list, portal screenshot and image clipboard`.

### Task 3: Overlay (`shot.rs`)
- [ ] Pure: `Selection::normalized`, `toolbar_position(sel, area, size)`, `window_at(rects, point)`; tests.
- [ ] Overlay view: dim layer with cut-out, mode bar, drag selection, size label, window/display hover outline, toolbar, keys (R/W/F, Enter, Ctrl+C, Ctrl+S, Esc), right click cancel.
- [ ] `toggle`: open overlays (Windows/X11/GNOME), interactive portal elsewhere.
- [ ] commit `feat(shot): capture a rectangle, window or screen`.

### Task 4: Output + wiring
- [ ] `shot_output.rs`: crop → PNG → save (folder, name) → clipboard.
- [ ] Daemon: `Toggle(Shot)`, tray entry (camera icon), `is_implemented`, `ImageClipboard` field.
- [ ] Settings section (switch, shortcut, folder row with Change/Open), i18n en/tr, icons, CLI help, README en/tr.
- [ ] `cargo test --workspace`, clippy; manual Windows run with debug build (`-WindowStyle Hidden`, installed daemon quit first, restarted after); commit, push, PR.
