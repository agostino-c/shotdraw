# shotdraw

Screenshot annotation tool for Sway/Wayland. Select a region, annotate it, copy to clipboard.

## Dependencies

- `slurp` — region selection
- `grim` — screenshot capture
- `wl-copy` — clipboard output
- A system font (resolved via `fc-match`; falls back to DejaVu/Liberation if unavailable)

## Usage

```
shotdraw
```

1. Drag to select a region with `slurp`
2. Annotate the screenshot
3. Press **Enter** to copy the result to clipboard
4. Press **Escape** to cancel without copying

### Flags

| Flag | Description |
|------|-------------|
| `-V`, `--version` | Print version and exit |

## Tools

| Tool | How it works |
|------|-------------|
| ⬜ Rect | Drag to draw a rectangle |
| ⭕ Circle | Drag to draw an ellipse from bounding box corners |
| ➡ Arrow | Drag to draw a line with an arrowhead at the tip |
| ✏ Text | Click to place, type, Enter to commit |

## Toolbar

- **Color** — 8 preset swatches (red, orange, yellow, green, blue, magenta, white, black)
- **Size** — stroke thickness 1–10px (shapes) or font size 8–96pt (text)

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| Enter | Copy annotated image to clipboard and exit |
| Escape | Cancel (or cancel in-progress text) |
| Ctrl+Z | Undo last annotation |

## Build

```
cargo build --release
```

Binary lands at `target/release/shotdraw`.

## Notes

The annotation window opens fullscreen and floating over your current Sway workspace, covering split layouts. Waybar is restored on exit.
