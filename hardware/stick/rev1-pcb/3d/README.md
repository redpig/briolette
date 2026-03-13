# Credstick Rev1 — 3D Printable Enclosure

Two-piece snap-fit case for the 60×30mm credstick PCB.

## Files

- `credstick-case.scad` — Parametric OpenSCAD source (edit this)
- `top-shell.stl` — Top shell (display, buttons, USB-C) — export from OpenSCAD
- `bottom-shell.stl` — Bottom shell (NFC antenna side) — export from OpenSCAD

## Printing

| Parameter | Value |
|-----------|-------|
| Material | PETG (recommended) or PLA |
| Layer height | 0.2mm |
| Infill | 20-30% |
| Walls | 2 perimeters minimum |
| Supports | Not needed (overhang-free design) |

## Features

- **Display window**: Rectangular cutout for 1.54" e-ink (30×22mm opening)
- **Button holes**: Two 4mm holes for tactile switch access
- **USB-C cutout**: Right end wall, full-height slot
- **Keyring hole**: Left end, aligned with PCB mounting hole
- **LED window**: Small 2mm hole for charge status LED
- **Piezo slot**: Side ventilation for cantilever vibration clearance
- **Snap-fit tabs**: Tool-free assembly, friction fit
- **PCB ledges**: Internal shelves support the PCB at the split plane

## Exporting STL

Open `credstick-case.scad` in OpenSCAD. Uncomment the desired module
at the bottom of the file, then **Render** (F6) and **Export as STL** (F7):

```
// For top shell:
top_shell();

// For bottom shell:
bottom_shell();

// For assembly preview:
pcb_mockup();
top_shell();
bottom_shell();
```

## Dimensions

| | mm |
|---|---|
| Outer length | ~63.6 |
| Outer width | ~33.6 |
| Outer height | ~9.6 |
| Wall thickness | 1.5 |
| Floor/roof | 1.0 |
| PCB tolerance | 0.3mm per side |
