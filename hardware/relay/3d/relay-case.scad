// ============================================================
// Briolette Solar Relay — 3D Printable Enclosure
// ============================================================
// Two-piece: top shell (solar cell window) + bottom shell (NFC tap zone)
// 70x45mm PCB, solar cell on top, NFC reader antenna on bottom
//
// Print: PETG, 0.2mm layers, 20% infill, no supports
// ============================================================

// ---- PCB dimensions ----
pcb_length = 70.0;
pcb_width  = 45.0;
pcb_thick  = 1.6;

// ---- Clearances ----
comp_clearance_top = 4.0;   // components (MCU, PN7150, etc.)
solar_clearance    = 2.0;   // solar cell sits above components
comp_clearance_bot = 1.5;   // bottom clearance (antenna traces)

// ---- Shell ----
wall = 1.8;
floor_thick = 1.2;          // thicker floor for NFC (no metal blocking)
roof_thick  = 0.8;          // thin roof under solar cell window
pcb_tol = 0.3;
corner_r = 2.5;

// ---- Derived ----
case_length = pcb_length + 2 * (wall + pcb_tol);
case_width  = pcb_width  + 2 * (wall + pcb_tol);
case_height_top = roof_thick + solar_clearance + comp_clearance_top + pcb_thick / 2;
case_height_bot = floor_thick + comp_clearance_bot + pcb_thick / 2;

split_z = 0;

// ---- Cutouts ----

// Solar cell window (top face, 40x30mm, offset toward upper-left)
solar_length = 42.0;   // slightly larger than cell for gasket
solar_width  = 32.0;
solar_offset_x = -8.0; // shifted left
solar_offset_y = -2.0;

// USB-C (bottom edge)
usbc_width  = 9.0;
usbc_height = 3.5;

// Button holes (4 buttons, left column)
btn_dia = 4.5;
btn_spacing = 7.0;
btn_x = -pcb_length / 2 + 7.0;
btn_y_start = -pcb_width / 2 + 10.0;

// LED windows (3 LEDs near buttons)
led_dia = 2.0;
led_x = -pcb_length / 2 + 12.0;
led_y = pcb_width / 2 - 10.0;
led_spacing = 3.0;

// NFC tap zone marking (bottom shell, silkscreen equivalent)
nfc_zone_x = 13.5;
nfc_zone_y = 0;
nfc_zone_length = 38.0;
nfc_zone_width  = 28.0;

// Screw bosses (M2 self-tapping, 4 corners)
screw_boss_dia = 5.0;
screw_hole_dia = 1.6;  // for M2 self-tap
screw_inset_x = 6.0;
screw_inset_y = 5.0;

// ============================================================
// Utility
// ============================================================

module rounded_box(size, r) {
    hull() {
        for (x = [-size[0]/2 + r, size[0]/2 - r])
            for (y = [-size[1]/2 + r, size[1]/2 - r])
                translate([x, y, 0])
                    cylinder(h = size[2], r = r, $fn = 32);
    }
}

// ============================================================
// Top Shell
// ============================================================
module top_shell() {
    difference() {
        // Outer
        translate([0, 0, split_z])
            rounded_box([case_length, case_width, case_height_top], corner_r);

        // Inner cavity
        translate([0, 0, split_z - 0.01])
            rounded_box([
                case_length - 2 * wall,
                case_width  - 2 * wall,
                case_height_top - roof_thick + 0.02
            ], corner_r - wall);

        // Solar cell window (rectangular cutout through roof)
        translate([solar_offset_x, solar_offset_y,
                   split_z + case_height_top - roof_thick - 0.01])
            rounded_box([solar_length, solar_width, roof_thick + 0.02], 1.5);

        // Button holes (4x, through roof)
        for (i = [0:3])
            translate([btn_x, btn_y_start + i * btn_spacing,
                       split_z + case_height_top - roof_thick - 0.01])
                cylinder(h = roof_thick + 0.02, d = btn_dia, $fn = 24);

        // LED windows (3x, through roof)
        for (i = [0:2])
            translate([led_x + i * led_spacing, led_y,
                       split_z + case_height_top - roof_thick - 0.01])
                cylinder(h = roof_thick + 0.02, d = led_dia, $fn = 16);

        // USB-C cutout (bottom edge wall)
        translate([-usbc_width / 2,
                   case_width / 2 - wall - 0.01,
                   split_z - usbc_height / 2])
            cube([usbc_width, wall + 0.02, usbc_height + case_height_top]);

        // Screw holes (through top shell corners)
        for (sx = [-1, 1])
            for (sy = [-1, 1])
                translate([sx * (pcb_length / 2 - screw_inset_x),
                           sy * (pcb_width / 2 - screw_inset_y),
                           split_z - 0.01])
                    cylinder(h = case_height_top + 0.02, d = screw_hole_dia, $fn = 16);
    }
}

// ============================================================
// Bottom Shell
// ============================================================
module bottom_shell() {
    difference() {
        // Outer
        translate([0, 0, split_z - case_height_bot])
            rounded_box([case_length, case_width, case_height_bot], corner_r);

        // Inner cavity
        translate([0, 0, split_z - case_height_bot + floor_thick - 0.01])
            rounded_box([
                case_length - 2 * wall,
                case_width  - 2 * wall,
                case_height_bot - floor_thick + 0.02
            ], corner_r - wall);

        // USB-C cutout (bottom half)
        translate([-usbc_width / 2,
                   case_width / 2 - wall - 0.01,
                   split_z - case_height_bot - 0.01])
            cube([usbc_width, wall + 0.02,
                  case_height_bot + usbc_height / 2 + 0.01]);
    }

    // Screw bosses (cylindrical posts)
    for (sx = [-1, 1])
        for (sy = [-1, 1])
            translate([sx * (pcb_length / 2 - screw_inset_x),
                       sy * (pcb_width / 2 - screw_inset_y),
                       split_z - case_height_bot + floor_thick])
                difference() {
                    cylinder(h = case_height_bot - floor_thick,
                             d = screw_boss_dia, $fn = 24);
                    translate([0, 0, -0.01])
                        cylinder(h = case_height_bot - floor_thick + 0.02,
                                 d = screw_hole_dia, $fn = 16);
                };

    // PCB support rails
    for (sign = [-1, 1])
        translate([0, sign * (pcb_width / 2 + pcb_tol - 0.5),
                   split_z - pcb_thick / 2 - 0.5])
            cube([pcb_length - 14, 1.0, 0.5], center = true);

    // NFC tap zone marker (raised ring on bottom face for haptic feedback)
    translate([nfc_zone_x, nfc_zone_y, split_z - case_height_bot - 0.3])
        difference() {
            rounded_box([nfc_zone_length + 2, nfc_zone_width + 2, 0.3], 2.0);
            translate([0, 0, -0.01])
                rounded_box([nfc_zone_length - 2, nfc_zone_width - 2, 0.32], 1.5);
        };
}

// ============================================================
// Assembly
// ============================================================

// Print layout: shells side by side, flat on build plate
translate([-case_length / 2 - 5, 0, case_height_top])
    rotate([180, 0, 0])
        top_shell();

translate([case_length / 2 + 5, 0, case_height_bot])
    bottom_shell();
