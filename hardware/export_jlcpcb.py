#!/usr/bin/env python3
"""
JLCPCB Manufacturing File Exporter for Briolette Hardware

Generates Gerber files, drill files, BOM, and CPL (centroid) for JLCPCB
ordering. Requires KiCad 8.0+ with the pcbnew Python API.

Usage:
    # From the hardware/ directory, with KiCad installed:
    python3 export_jlcpcb.py

    # Or export a single board:
    python3 export_jlcpcb.py stick
    python3 export_jlcpcb.py relay

Output:
    stick/rev1-pcb/gerbers/briolette-credstick-jlcpcb.zip
    relay/gerbers/briolette-relay-jlcpcb.zip

Each zip contains:
    - Gerber files for all copper, mask, silk, paste, and edge layers
    - Excellon drill files (PTH + NPTH)
    - BOM CSV (JLCPCB format)
    - CPL/centroid CSV (JLCPCB format)
"""

import os
import sys
import csv
import zipfile
import shutil
from pathlib import Path

try:
    import pcbnew
except ImportError:
    print("ERROR: pcbnew Python module not found.")
    print("This script must be run with KiCad's Python environment.")
    print()
    print("Options:")
    print("  1. Run from KiCad's scripting console")
    print("  2. Use KiCad's bundled Python:")
    print("     Linux:   /usr/bin/python3 export_jlcpcb.py")
    print("     macOS:   /Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3 export_jlcpcb.py")
    print("     Windows: C:\\Program Files\\KiCad\\8.0\\bin\\python.exe export_jlcpcb.py")
    sys.exit(1)


# Board configurations
BOARDS = {
    "stick": {
        "pcb_path": "stick/rev1-pcb/kicad/stick.kicad_pcb",
        "output_dir": "stick/rev1-pcb/gerbers",
        "zip_name": "briolette-credstick-jlcpcb.zip",
        "description": "Briolette Credstick Rev1 (60x30mm, 4-layer)",
    },
    "relay": {
        "pcb_path": "relay/kicad/relay.kicad_pcb",
        "output_dir": "relay/gerbers",
        "zip_name": "briolette-relay-jlcpcb.zip",
        "description": "Briolette Solar Relay (70x45mm, 4-layer)",
    },
}

# JLCPCB gerber layer mapping
# Maps KiCad layer IDs to JLCPCB-expected file extensions
GERBER_LAYERS = {
    pcbnew.F_Cu: ("F_Cu", "Front Copper"),
    pcbnew.In1_Cu: ("In1_Cu", "Inner Layer 1 (GND)"),
    pcbnew.In2_Cu: ("In2_Cu", "Inner Layer 2 (PWR)"),
    pcbnew.B_Cu: ("B_Cu", "Back Copper"),
    pcbnew.F_Paste: ("F_Paste", "Front Paste"),
    pcbnew.B_Paste: ("B_Paste", "Back Paste"),
    pcbnew.F_SilkS: ("F_Silkscreen", "Front Silkscreen"),
    pcbnew.B_SilkS: ("B_Silkscreen", "Back Silkscreen"),
    pcbnew.F_Mask: ("F_Mask", "Front Soldermask"),
    pcbnew.B_Mask: ("B_Mask", "Back Soldermask"),
    pcbnew.Edge_Cuts: ("Edge_Cuts", "Board Outline"),
}


def export_gerbers(board, output_dir):
    """Export Gerber files for all required layers."""
    plot_controller = pcbnew.PLOT_CONTROLLER(board)
    plot_options = plot_controller.GetPlotOptions()

    # Configure plot options for JLCPCB
    plot_options.SetOutputDirectory(output_dir)
    plot_options.SetPlotFrameRef(False)
    plot_options.SetSketchPadLineWidth(pcbnew.FromMM(0.1))
    plot_options.SetAutoScale(False)
    plot_options.SetScale(1)
    plot_options.SetMirror(False)
    plot_options.SetUseGerberAttributes(True)
    plot_options.SetUseGerberProtelExtensions(True)
    plot_options.SetUseAuxOrigin(False)
    plot_options.SetSubtractMaskFromSilk(True)
    plot_options.SetDrillMarksType(0)  # No drill marks on gerbers

    gerber_files = []

    for layer_id, (suffix, description) in GERBER_LAYERS.items():
        plot_controller.OpenPlotfile(suffix, pcbnew.PLOT_FORMAT_GERBER, description)
        plot_controller.SetLayer(layer_id)
        plot_controller.PlotLayer()
        plot_controller.ClosePlot()
        gerber_files.append(plot_controller.GetPlotFileName())
        print(f"  Exported: {suffix} ({description})")

    return gerber_files


def export_drill(board, output_dir):
    """Export Excellon drill files (PTH and NPTH separately, as JLCPCB requires)."""
    drill_writer = pcbnew.EXCELLON_WRITER(board)

    drill_writer.SetOptions(
        aMirror=False,
        aMinimalHeader=False,
        aOffset=pcbnew.VECTOR2I(0, 0),
        aMerge_PTH_NPTH=False,  # JLCPCB wants separate files
    )
    drill_writer.SetFormat(
        aMetric=True,
        aZerosFmt=pcbnew.EXCELLON_WRITER.DECIMAL_FORMAT,
    )
    drill_writer.CreateDrillandMapFilesSet(output_dir, True, False)

    drill_files = []
    for ext in ["-PTH.drl", "-NPTH.drl", ".drl"]:
        for f in Path(output_dir).glob(f"*{ext}"):
            drill_files.append(str(f))
            print(f"  Exported: {f.name}")

    return drill_files


def export_bom(board, output_dir, board_name):
    """
    Export BOM in JLCPCB format.

    JLCPCB BOM format requires columns:
        Comment, Designator, Footprint, LCSC Part #
    """
    bom_path = os.path.join(output_dir, f"{board_name}-BOM-JLCPCB.csv")

    # Group components by value + footprint
    components = {}
    for fp in board.GetFootprints():
        ref = fp.GetReference()
        if not ref:
            continue
        value = fp.GetValue() or ""
        fpid = fp.GetFPID()
        footprint = fpid.GetUniStringLibItemName() if fpid else ""

        # Skip non-component items (mounting holes, fiducials, test points)
        if ref.startswith("H") or ref.startswith("TP") or ref.startswith("FID"):
            continue

        key = (value, footprint)
        if key not in components:
            components[key] = {
                "comment": value,
                "designators": [],
                "footprint": footprint,
                "lcsc": "",  # User fills in LCSC part numbers
            }
        components[key]["designators"].append(ref)

    with open(bom_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["Comment", "Designator", "Footprint", "LCSC Part #"])
        for key in sorted(components.keys()):
            comp = components[key]
            designators = ",".join(sorted(comp["designators"],
                                          key=lambda r: (r[0], int("".join(filter(str.isdigit, r)) or "0"))))
            writer.writerow([
                comp["comment"],
                designators,
                comp["footprint"],
                comp["lcsc"],
            ])

    print(f"  Exported: {os.path.basename(bom_path)} ({len(components)} unique parts)")
    return bom_path


def export_cpl(board, output_dir, board_name):
    """
    Export CPL (Component Placement List / Centroid) in JLCPCB format.

    JLCPCB CPL format requires columns:
        Designator, Val, Package, Mid X, Mid Y, Rotation, Layer
    """
    cpl_path = os.path.join(output_dir, f"{board_name}-CPL-JLCPCB.csv")

    with open(cpl_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["Designator", "Val", "Package", "Mid X", "Mid Y", "Rotation", "Layer"])

        for fp in board.GetFootprints():
            ref = fp.GetReference()
            if not ref:
                continue

            # Skip non-component items
            if ref.startswith("H") or ref.startswith("TP") or ref.startswith("FID"):
                continue

            # Skip through-hole-only components (supercapacitors, piezo)
            # These are hand-soldered after PCBA
            if fp.HasOnlySMDPads() is False and fp.GetPadCount() > 0:
                # Check if it has any SMD pads at all
                pads = fp.Pads()
                has_smd = False
                if pads:
                    has_smd = any(pad.GetAttribute() == pcbnew.PAD_ATTRIB_SMD
                                 for pad in pads)
                if not has_smd:
                    print(f"  Skipping THT component: {ref} ({fp.GetValue()})")
                    continue

            pos = fp.GetPosition()
            if not pos:
                print(f"  WARNING: Skipping {ref} - no position data")
                continue
            x_mm = pcbnew.ToMM(pos.x)
            y_mm = -pcbnew.ToMM(pos.y)  # KiCad Y is inverted vs JLCPCB
            rotation = fp.GetOrientationDegrees()
            layer = "Top" if fp.GetLayer() == pcbnew.F_Cu else "Bottom"

            fpid = fp.GetFPID()
            package = fpid.GetUniStringLibItemName() if fpid else ""

            writer.writerow([
                ref,
                fp.GetValue() or "",
                package,
                f"{x_mm:.4f}mm",
                f"{y_mm:.4f}mm",
                f"{rotation:.1f}",
                layer,
            ])

    print(f"  Exported: {os.path.basename(cpl_path)}")
    return cpl_path


def create_zip(output_dir, zip_name, gerber_files, drill_files, bom_path, cpl_path):
    """Package all manufacturing files into a single zip for JLCPCB upload."""
    zip_path = os.path.join(output_dir, zip_name)

    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as zf:
        # Add gerber files
        for f in gerber_files:
            zf.write(f, os.path.basename(f))

        # Add drill files
        for f in drill_files:
            zf.write(f, os.path.basename(f))

        # Add BOM and CPL
        zf.write(bom_path, os.path.basename(bom_path))
        zf.write(cpl_path, os.path.basename(cpl_path))

    print(f"  Created: {zip_path}")
    return zip_path


def process_board(name, config, base_dir):
    """Process a single board: export all files and create zip."""
    pcb_path = os.path.join(base_dir, config["pcb_path"])
    output_dir = os.path.join(base_dir, config["output_dir"])

    print(f"\n{'='*60}")
    print(f"Processing: {config['description']}")
    print(f"PCB file:   {pcb_path}")
    print(f"Output:     {output_dir}")
    print(f"{'='*60}")

    # Verify PCB file exists
    if not os.path.exists(pcb_path):
        print(f"ERROR: PCB file not found: {pcb_path}")
        return None

    # Create output directory
    os.makedirs(output_dir, exist_ok=True)

    # Load the board
    print("\nLoading board...")
    board = pcbnew.LoadBoard(pcb_path)

    # Export gerbers
    print("\nExporting Gerber files...")
    gerber_files = export_gerbers(board, output_dir)

    # Export drill files
    print("\nExporting drill files...")
    drill_files = export_drill(board, output_dir)

    # Export BOM
    print("\nExporting BOM (JLCPCB format)...")
    bom_path = export_bom(board, output_dir, name)

    # Export CPL / centroid
    print("\nExporting CPL/centroid (JLCPCB format)...")
    cpl_path = export_cpl(board, output_dir, name)

    # Create zip
    print("\nCreating JLCPCB zip package...")
    zip_path = create_zip(output_dir, config["zip_name"], gerber_files,
                          drill_files, bom_path, cpl_path)

    return zip_path


def main():
    # Determine base directory (hardware/)
    script_dir = os.path.dirname(os.path.abspath(__file__))
    base_dir = script_dir  # Script lives in hardware/

    # Determine which boards to process
    if len(sys.argv) > 1:
        targets = [arg.lower() for arg in sys.argv[1:]]
        for t in targets:
            if t not in BOARDS:
                print(f"ERROR: Unknown board '{t}'. Available: {', '.join(BOARDS.keys())}")
                sys.exit(1)
    else:
        targets = list(BOARDS.keys())

    print("Briolette JLCPCB Manufacturing File Exporter")
    print(f"Boards to process: {', '.join(targets)}")

    results = {}
    for name in targets:
        zip_path = process_board(name, BOARDS[name], base_dir)
        results[name] = zip_path

    # Summary
    print(f"\n{'='*60}")
    print("EXPORT COMPLETE")
    print(f"{'='*60}")
    for name, zip_path in results.items():
        if zip_path:
            print(f"  {name}: {zip_path}")
        else:
            print(f"  {name}: FAILED")

    print()
    print("Next steps:")
    print("  1. Go to https://jlcpcb.com and click 'Order Now'")
    print("  2. Upload the zip file(s)")
    print("  3. Fill in LCSC part numbers in the BOM CSV")
    print("  4. See JLCPCB-ORDERING-GUIDE.md for detailed instructions")


if __name__ == "__main__":
    main()
