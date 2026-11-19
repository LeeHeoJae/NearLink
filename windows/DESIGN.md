---
version: alpha
name: NearLink Windows
colors:
  primary: "#1559CE"
  background: "#F3F3F6"
  surface: "#FFFFFF"
  text: "#17171B"
  secondary: "#595961"
typography:
  body-md:
    fontFamily: Segoe UI
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.5
rounded:
  sm: 4px
spacing:
  md: 16px
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "#FFFFFF"
---

## Overview

Operate mode: a small resident battery receiver for a person working at a Windows PC.
The user pins Fluent Design / Windows 11 conventions and the Android NearLink identity.
Keep native Rust + Win32, OS title bars, system menus, keyboard navigation and UI Automation.
No browser engine, framework runtime, wallpaper-derived brand color or decorative motion.

## Colors

Use Android NearLink blue #1559CE for primary actions and battery values; dark blue accent
#9BBFFF with #002F77 text. Follow AppsUseLightTheme and live system theme changes.
High contrast takes precedence and uses Windows system colors, including selection text.
Connection, charging and stale data always have text equivalents.

## Typography

Segoe UI on the Fluent ramp: 12 caption, 14 body, 14 semibold device name, 20 semibold
battery, 24 semibold page heading. Battery glyphs use Segoe Fluent Icons (MDL2 Assets fallback).
Scale layout and fonts with per-monitor DPI and Windows Accessibility TextScaleFactor.
Do not replace OS title bars or their accessible minimize/maximize/close controls.

## Layout

Main window starts at 720 by 760 logical units; minimum 620 by 520.
The device list fills the available height. Each 68-unit row follows the Windows Settings
device card: battery glyph, name over a compact status line (connection · freshness · age),
and the reading with a charging label on the right. Scroll vertically for more devices.
Margins: 24 main content, 16 list. No table header or horizontal scrolling.
A selected device has explicit actions below the list plus a native context menu.
Tray left click opens a 430-unit quick window, capped to the monitor work area, with
all devices scrollable, add and full settings access. Right click keeps the native menu.

## Elevation & Depth

Opaque surfaces; no blur or extra compositor animations. OS owns window shadows.
Selection has a thin outline; unselected rows are quiet white/dark surfaces.

## Shapes

Request the small DWM window corner preference. Controls and rows remain restrained
rectangles. High contrast disables requested rounding. No pill-shaped containers.

## Components

Native list view with custom drawing preserves a full accessible text string per device,
selection, arrow keys, scroll and focus. Native buttons retain names, mnemonics and roles.
Add device uses a separate owned window: scanning/pairing needs protected focus.
Alias and notifications/startup also use short owned dialogs. Existing copy is retained.
15 minutes without battery data marks an old value, independently of BLE connection.
Disconnects show connection lost / retry pending, paused reception and unread values are explicit.
Preview mode is opt-in, requires an isolated data directory, labels example data,
never starts BLE, saves fixtures, changes startup or raises battery notifications.

## Do's and Don'ts

Preserve NearLink blue, concise copy in every language (English is the source text, Korean the first translation) and Android information order. Leave room in fixed layouts for longer translations.
Use native system menus and controls, system contrast colors and visible keyboard focus.
Avoid giant metrics, decorative icons, nested cards, excessive rounding and color-only states.
The battery glyph is informational; lost connections and old values use caution text color
(#9D5D00 light, #FCE100 dark) next to their words, and muted readings dim to secondary.

## Visual Contract

- Surface: native Windows platform app, Operate.
- User job: scan battery and connection state, then act on one device.
- Reference family: user-pinned Windows 11 Fluent conventions.
- Design source: user brief, Android BridgeTheme / BridgeScreen, incumbent Win32 behavior.
- Density: compact multi-line device list, one column.
- Layout: heading, add/settings, device list, selection detail/actions, tray hint.
- CRUD create mode: separate owned dialog for BLE discovery and pairing.
- Required states: empty, no selection, selected/focused, charging, unread, disconnected,
  paused, stale, scanning, settings validation, light, dark, high contrast, scaled text.
- Forbidden tells: report table, web-style navigation, hero metrics, glass/blur, pill cards.
- Verification: compiled native app, screenshot inspection, keyboard/interaction checks,
  isolated fixtures for states, matching release binary and idle resource comparison.
