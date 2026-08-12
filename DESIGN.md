# Design System

## Theme

Native Windows product UI using a restrained black-and-white Material vocabulary. Near-black is the canvas, stepped neutral surfaces create hierarchy, and white is reserved for primary actions and high-priority status.

## Color

- Background: grayscale tone 6
- Container: grayscale tone 10
- Raised/control surface: grayscale tone 18
- Outline: grayscale tone 24
- Primary text/action: grayscale tone 96
- Secondary text: grayscale tone 68
- Muted text: grayscale tone 60

All body and control text must meet WCAG AA contrast, enforced by Rust tests.

## Typography

Segoe UI Variable Text with Segoe UI fallback. One compact product scale: 24 title, 18 section/status heading, 15 body, 13 supporting label, 22 metric, 14 control label.

## Shape and spacing

Use a 4-pixel spacing foundation with 24-pixel outer gutters, 12-16-pixel surface radii, and full-pill 40-44-pixel action controls. Do not nest cards. Group related metrics in one segmented container.

## Components

- Borderless rounded application shell with a draggable header
- High-priority status panel with progress indicator
- Segmented metrics strip
- Native-backed source list with selection, scrolling, and clear empty state
- Filled primary and neutral secondary action buttons
- Inline album-name editor for the selected folder

## Interaction

The application remains open when focus moves elsewhere and hides only through Close, Escape, or the tray action. The header drags the window. Actions expose default, hover, focus, pressed, disabled, running, success, and actionable error states.
