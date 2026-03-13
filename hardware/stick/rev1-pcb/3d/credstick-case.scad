// ============================================================
// Briolette Credstick Rev1 — 3D Printable Enclosure
// ============================================================
// Target: FDM printing (PLA/PETG), 0.2mm layer height
// Two-piece snap-fit: top shell + bottom shell
// PCB: 60mm x 30mm, 1.6mm thick, 4-layer
//
// Print settings:
//   Material: PETG recommended (impact resistance, slight flex)
//   Layer height: 0.2mm
//   Infill: 20-30%
//   Supports: minimal (overhang-free design)
//   Walls: 2 perimeters minimum
// ============================================================

// ---- Parameters (edit these to tune fit) ----

// PCB dimensions
pcb_length = 60.0;       // X axis
pcb_width  = 30.0;       // Y axis
pcb_thick  = 1.6;        // PCB thickness

// Component clearances (above PCB top face)
comp_clearance_top = 5.0; // tallest top-side component (supercaps ~14mm dia, mounted standing)
comp_clearance_bot = 1.5; // bottom-side clearance (NFC antenna trace, no components)

// Wall thickness
wall = 1.5;               // side walls
floor_thick = 1.0;        // bottom shell floor
roof_thick  = 1.0;        // top shell roof

// Tolerances
pcb_tol = 0.3;            // gap around PCB edges (per side)
snap_tol = 0.1;           // snap-fit interference

// Corner radius (matches PCB outline)
corner_r = 2.0;

// Case outer dimensions (derived)
case_length = pcb_length + 2 * (wall + pcb_tol);
case_width  = pcb_width  + 2 * (wall + pcb_tol);
case_height_top = roof_thick + comp_clearance_top + pcb_thick / 2;
case_height_bot = floor_thick + comp_clearance_bot + pcb_thick / 2;
case_height = case_height_top + case_height_bot;

// Split plane: Z=0 at PCB midplane
split_z = 0;

// ---- Cutout dimensions ----

// USB-C connector: right end, centered
usbc_width  = 9.0;       // connector opening width
usbc_height = 3.5;       // connector opening height
usbc_offset_z = 0;       // center relative to PCB midplane

// E-ink display window: upper-left area of top shell
// GDEY0154D67: 1.54" = ~37.3mm diagonal, active area ~27.6x27.6mm
display_length = 30.0;   // window X
display_width  = 22.0;   // window Y
display_offset_x = -5.0; // shift left from center (display area)
display_offset_y = 0.0;  // centered in Y

// Buttons: two tactile switches, accessible through top shell
btn_dia = 4.0;           // button hole diameter
btn1_x = -15.0;          // left button X (from center)
btn2_x = -5.0;           // right button X
btn_y  = 12.0;           // Y offset (below center, near bottom edge)

// Keyring hole: left end, 3.2mm through-hole
keyring_dia = 4.0;       // hole through case (slightly larger than PCB hole)
keyring_x = -pcb_length / 2 + 5.0;

// LED window: small hole for charge status LED
led_dia = 2.0;
led_x = 10.0;            // approximate LED position
led_y = -10.0;

// Piezo slot: side opening for cantilever vibration
piezo_slot_length = 28.0;
piezo_slot_width  = 2.0;
piezo_slot_x = -8.0;     // centered on piezo area

// ---- Snap-fit tab dimensions ----
snap_tab_length = 8.0;
snap_tab_height = 1.5;
snap_tab_depth  = 0.8;

// ============================================================
// Utility modules
// ============================================================

module rounded_box(size, r) {
    // Rounded-corner box (XY plane corners rounded, Z axis straight)
    hull() {
        for (x = [-size[0]/2 + r, size[0]/2 - r])
            for (y = [-size[1]/2 + r, size[1]/2 - r])
                translate([x, y, 0])
                    cylinder(h = size[2], r = r, $fn = 32);
    }
}

module rounded_box_centered(size, r) {
    translate([0, 0, -size[2]/2])
        rounded_box(size, r);
}

// ============================================================
// Top Shell (display side, buttons, USB-C top half)
// ============================================================
module top_shell() {
    difference() {
        // Outer shell
        translate([0, 0, split_z])
            rounded_box([case_length, case_width, case_height_top], corner_r);

        // Inner cavity (hollow out)
        translate([0, 0, split_z - 0.01])
            rounded_box([
                case_length - 2 * wall,
                case_width  - 2 * wall,
                case_height_top - roof_thick + 0.02
            ], corner_r - wall);

        // Display window (rectangular cutout through roof)
        translate([display_offset_x, display_offset_y,
                   split_z + case_height_top - roof_thick - 0.01])
            rounded_box([display_length, display_width, roof_thick + 0.02], 1.0);

        // Button holes (through roof)
        for (bx = [btn1_x, btn2_x])
            translate([bx, btn_y, split_z + case_height_top - roof_thick - 0.01])
                cylinder(h = roof_thick + 0.02, d = btn_dia, $fn = 24);

        // USB-C cutout (right end wall, top half)
        translate([case_length / 2 - wall - 0.01,
                   -usbc_width / 2,
                   split_z + usbc_offset_z - usbc_height / 2])
            cube([wall + 0.02, usbc_width, usbc_height / 2 + case_height_top]);

        // LED window (small hole through roof)
        translate([led_x, led_y, split_z + case_height_top - roof_thick - 0.01])
            cylinder(h = roof_thick + 0.02, d = led_dia, $fn = 16);

        // Keyring hole (through left end wall + extends into shell)
        translate([keyring_x, 0, split_z - 1])
            cylinder(h = case_height_top + 2, d = keyring_dia, $fn = 32);
    }

    // Snap-fit receptacle ridges (inside walls, for bottom shell tabs)
    // Left and right side walls
    for (sign = [-1, 1])
        translate([0, sign * (case_width / 2 - wall), split_z])
            cube([snap_tab_length, snap_tab_depth, snap_tab_height], center = true);
}

// ============================================================
// Bottom Shell (NFC antenna side, flat)
// ============================================================
module bottom_shell() {
    difference() {
        // Outer shell
        translate([0, 0, split_z - case_height_bot])
            rounded_box([case_length, case_width, case_height_bot], corner_r);

        // Inner cavity
        translate([0, 0, split_z - case_height_bot + floor_thick - 0.01])
            rounded_box([
                case_length - 2 * wall,
                case_width  - 2 * wall,
                case_height_bot - floor_thick + 0.02
            ], corner_r - wall);

        // USB-C cutout (right end wall, bottom half)
        translate([case_length / 2 - wall - 0.01,
                   -usbc_width / 2,
                   split_z - case_height_bot - 0.01])
            cube([wall + 0.02, usbc_width,
                  case_height_bot + usbc_offset_z + usbc_height / 2 + 0.01]);

        // Keyring hole
        translate([keyring_x, 0, split_z - case_height_bot - 1])
            cylinder(h = case_height_bot + 2, d = keyring_dia, $fn = 32);

        // Piezo slot (side wall ventilation for cantilever movement)
        translate([piezo_slot_x, -case_width / 2 - 0.01,
                   split_z - case_height_bot + floor_thick])
            cube([piezo_slot_length, wall + 0.02, piezo_slot_width]);
    }

    // PCB support ledges (internal shelves the PCB rests on)
    for (sign = [-1, 1])
        translate([0, sign * (pcb_width / 2 + pcb_tol - 0.5),
                   split_z - pcb_thick / 2 - 0.5])
            cube([pcb_length - 10, 1.0, 0.5], center = true);

    // Snap-fit tabs (hook onto top shell ridges)
    for (sign = [-1, 1])
        translate([0, sign * (case_width / 2 - wall - snap_tol),
                   split_z - snap_tab_height])
            difference() {
                cube([snap_tab_length, snap_tab_depth + snap_tol,
                      snap_tab_height], center = true);
                // Chamfer for easy insertion
                translate([0, sign * snap_tab_depth / 2, -snap_tab_height / 2])
                    rotate([sign * 30, 0, 0])
                        cube([snap_tab_length + 0.1, snap_tab_depth, snap_tab_height],
                             center = true);
            }
}

// ============================================================
// PCB mockup (for visualization, not printed)
// ============================================================
module pcb_mockup() {
    color("green", 0.5)
        translate([0, 0, -pcb_thick / 2])
            rounded_box([pcb_length, pcb_width, pcb_thick], corner_r);
}

// ============================================================
// Assembly / rendering
// ============================================================

// Uncomment ONE of the following to render/export:

// -- Full assembly (for visualization) --
// pcb_mockup();
// color("DarkSlateGray", 0.8) top_shell();
// color("DarkSlateGray", 0.6) bottom_shell();

// -- Top shell only (for STL export) --
// top_shell();

// -- Bottom shell only (for STL export) --
// bottom_shell();

// -- Print layout: both shells side by side, flat on build plate --
translate([-case_length / 2 - 5, 0, case_height_top])
    rotate([180, 0, 0])
        top_shell();

translate([case_length / 2 + 5, 0, case_height_bot])
    rotate([0, 0, 0])
        bottom_shell();
