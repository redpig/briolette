#!/usr/bin/env python3
"""
JLCPCB Manufacturing File Exporter for Briolette Hardware

Generates Gerber files, drill files, BOM, and CPL (centroid) for JLCPCB
ordering. Uses kicad-cli (KiCad 8.0+ / 9.x) -- no pcbnew Python bindings required.

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
import re
import sys
import csv
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path


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

# 4-layer gerber layers for JLCPCB
GERBER_LAYERS = "F.Cu,In1.Cu,In2.Cu,B.Cu,F.Paste,B.Paste,F.Silkscreen,B.Silkscreen,F.Mask,B.Mask,Edge.Cuts"

# Prefixes for non-component footprints (mounting holes, test points, fiducials)
SKIP_PREFIXES = ("H", "TP", "FID")


def find_kicad_cli():
    """Find kicad-cli executable."""
    # Check PATH first
    cli = shutil.which("kicad-cli")
    if cli:
        return cli

    # Common install locations
    candidates = [
        "/usr/bin/kicad-cli",
        "/usr/local/bin/kicad-cli",
        "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli",
        r"C:\Program Files\KiCad\8.0\bin\kicad-cli.exe",
    ]
    for c in candidates:
        if os.path.isfile(c):
            return c

    return None


def run_kicad_cli(args):
    """Run a kicad-cli command, raising on failure."""
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"  ERROR: kicad-cli failed (exit {result.returncode})")
        if result.stdout:
            print(f"  stdout: {result.stdout.strip()}")
        if result.stderr:
            # Filter wx debug noise, show actual errors
            lines = result.stderr.strip().split("\n")
            errors = [l for l in lines if "Adding duplicate image handler" not in l]
            if errors:
                print(f"  stderr: {chr(10).join(errors)}")
        raise RuntimeError(f"kicad-cli failed: {' '.join(args)}")
    return result


def export_gerbers(kicad_cli, pcb_path, output_dir):
    """Export Gerber files using kicad-cli."""
    run_kicad_cli([
        kicad_cli, "pcb", "export", "gerbers",
        "--output", output_dir + "/",
        "--layers", GERBER_LAYERS,
        "--subtract-soldermask",
        "--no-protel-ext",
        "--use-drill-file-origin",
        pcb_path,
    ])
    gerber_files = list(Path(output_dir).glob("*.g*")) + list(Path(output_dir).glob("*.G*"))
    for f in gerber_files:
        print(f"  Exported: {f.name}")
    return [str(f) for f in gerber_files]


def export_drill(kicad_cli, pcb_path, output_dir):
    """Export Excellon drill files (PTH + NPTH separately)."""
    run_kicad_cli([
        kicad_cli, "pcb", "export", "drill",
        "--output", output_dir + "/",
        "--format", "excellon",
        "--excellon-separate-th",
        "--generate-map", "--map-format", "gerberx2",
        pcb_path,
    ])
    drill_files = list(Path(output_dir).glob("*.drl"))
    for f in drill_files:
        print(f"  Exported: {f.name}")
    return [str(f) for f in drill_files]


def export_cpl(kicad_cli, pcb_path, output_dir, board_name):
    """Export CPL/centroid in JLCPCB format using kicad-cli pos export."""
    cpl_path = os.path.join(output_dir, f"{board_name}-CPL-JLCPCB.csv")

    # kicad-cli exports position file; we reformat for JLCPCB
    with tempfile.NamedTemporaryFile(suffix=".csv", delete=False, mode="r") as tmp:
        tmp_path = tmp.name

    try:
        run_kicad_cli([
            kicad_cli, "pcb", "export", "pos",
            "--output", tmp_path,
            "--format", "csv",
            "--units", "mm",
            "--smd-only",
            pcb_path,
        ])

        # Read kicad-cli output and reformat to JLCPCB columns
        with open(tmp_path, "r") as infile, open(cpl_path, "w", newline="") as outfile:
            reader = csv.DictReader(infile)
            writer = csv.writer(outfile)
            writer.writerow(["Designator", "Val", "Package", "Mid X", "Mid Y", "Rotation", "Layer"])

            for row in reader:
                ref = row.get("Ref", "")
                if ref.startswith(SKIP_PREFIXES):
                    continue
                writer.writerow([
                    ref,
                    row.get("Val", ""),
                    row.get("Package", ""),
                    f"{row.get('PosX', '0')}mm",
                    f"{row.get('PosY', '0')}mm",
                    row.get("Rot", "0"),
                    "Top" if row.get("Side", "top").lower() == "top" else "Bottom",
                ])
    finally:
        os.unlink(tmp_path)

    print(f"  Exported: {os.path.basename(cpl_path)}")
    return cpl_path


def parse_footprints_from_pcb(pcb_path):
    """
    Parse footprint data directly from .kicad_pcb S-expression file.
    Avoids pcbnew SWIG bindings entirely.
    """
    with open(pcb_path, "r") as f:
        content = f.read()

    footprints = []
    # Match top-level footprint blocks
    # Each footprint starts with (footprint "..." and we need ref + value properties
    fp_pattern = re.compile(r'\(footprint\s+"([^"]*)"', re.MULTILINE)
    # KiCad 7: (fp_text reference "R1"), KiCad 8+: (property "Reference" "R1")
    ref_pattern = re.compile(r'(?:\(fp_text\s+reference\s+"([^"]*)"|\(property\s+"Reference"\s+"([^"]*)")')
    val_pattern = re.compile(r'(?:\(fp_text\s+value\s+"([^"]*)"|\(property\s+"Value"\s+"([^"]*)")')

    # Walk the file finding footprint blocks by tracking parenthesis depth
    i = 0
    while i < len(content):
        match = fp_pattern.search(content, i)
        if not match:
            break

        fp_name = match.group(1)
        # Find the extent of this footprint block by counting parens
        start = match.start()
        depth = 0
        j = start
        while j < len(content):
            if content[j] == "(":
                depth += 1
            elif content[j] == ")":
                depth -= 1
                if depth == 0:
                    break
            j += 1

        fp_block = content[start:j + 1]

        ref_match = ref_pattern.search(fp_block)
        val_match = val_pattern.search(fp_block)

        ref = (ref_match.group(1) or ref_match.group(2)) if ref_match else ""
        value = (val_match.group(1) or val_match.group(2)) if val_match else ""

        if ref and not ref.startswith(SKIP_PREFIXES):
            footprints.append({
                "ref": ref,
                "value": value,
                "footprint": fp_name,
            })

        i = j + 1

    return footprints


def export_bom(pcb_path, output_dir, board_name):
    """Export BOM in JLCPCB format by parsing the .kicad_pcb file directly."""
    bom_path = os.path.join(output_dir, f"{board_name}-BOM-JLCPCB.csv")

    footprints = parse_footprints_from_pcb(pcb_path)

    # Group by value + footprint
    components = {}
    for fp in footprints:
        key = (fp["value"], fp["footprint"])
        if key not in components:
            components[key] = {
                "comment": fp["value"],
                "designators": [],
                "footprint": fp["footprint"],
                "lcsc": "",
            }
        components[key]["designators"].append(fp["ref"])

    with open(bom_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["Comment", "Designator", "Footprint", "LCSC Part #"])
        for key in sorted(components.keys()):
            comp = components[key]
            designators = ",".join(sorted(
                comp["designators"],
                key=lambda r: (r[0], int("".join(filter(str.isdigit, r)) or "0")),
            ))
            writer.writerow([comp["comment"], designators, comp["footprint"], comp["lcsc"]])

    print(f"  Exported: {os.path.basename(bom_path)} ({len(components)} unique parts)")
    return bom_path


def create_zip(output_dir, zip_name, gerber_files, drill_files, bom_path, cpl_path):
    """Package all manufacturing files into a single zip for JLCPCB upload."""
    zip_path = os.path.join(output_dir, zip_name)

    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as zf:
        for f in gerber_files + drill_files:
            zf.write(f, os.path.basename(f))
        zf.write(bom_path, os.path.basename(bom_path))
        zf.write(cpl_path, os.path.basename(cpl_path))

    print(f"  Created: {zip_path}")
    return zip_path


def process_board(kicad_cli, name, config, base_dir):
    """Process a single board: export all files and create zip."""
    pcb_path = os.path.join(base_dir, config["pcb_path"])
    output_dir = os.path.join(base_dir, config["output_dir"])

    print(f"\n{'='*60}")
    print(f"Processing: {config['description']}")
    print(f"PCB file:   {pcb_path}")
    print(f"Output:     {output_dir}")
    print(f"{'='*60}")

    if not os.path.exists(pcb_path):
        print(f"ERROR: PCB file not found: {pcb_path}")
        return None

    os.makedirs(output_dir, exist_ok=True)

    print("\nExporting Gerber files...")
    gerber_files = export_gerbers(kicad_cli, pcb_path, output_dir)

    print("\nExporting drill files...")
    drill_files = export_drill(kicad_cli, pcb_path, output_dir)

    print("\nExporting BOM (JLCPCB format)...")
    bom_path = export_bom(pcb_path, output_dir, name)

    print("\nExporting CPL/centroid (JLCPCB format)...")
    cpl_path = export_cpl(kicad_cli, pcb_path, output_dir, name)

    print("\nCreating JLCPCB zip package...")
    zip_path = create_zip(output_dir, config["zip_name"], gerber_files,
                          drill_files, bom_path, cpl_path)

    return zip_path


def main():
    kicad_cli = find_kicad_cli()
    if not kicad_cli:
        print("ERROR: kicad-cli not found.")
        print("This script requires KiCad 8.0+ with kicad-cli in PATH.")
        print()
        print("Install KiCad 8.0+ or add kicad-cli to your PATH.")
        sys.exit(1)

    script_dir = os.path.dirname(os.path.abspath(__file__))
    base_dir = script_dir

    if len(sys.argv) > 1:
        targets = [arg.lower() for arg in sys.argv[1:]]
        for t in targets:
            if t not in BOARDS:
                print(f"ERROR: Unknown board '{t}'. Available: {', '.join(BOARDS.keys())}")
                sys.exit(1)
    else:
        targets = list(BOARDS.keys())

    # Print kicad-cli version for diagnostics
    ver_result = subprocess.run([kicad_cli, "version"], capture_output=True, text=True)
    kicad_version = ver_result.stdout.strip() if ver_result.returncode == 0 else "unknown"

    print("Briolette JLCPCB Manufacturing File Exporter")
    print(f"Using: {kicad_cli} (version {kicad_version})")
    print(f"Boards to process: {', '.join(targets)}")

    results = {}
    for name in targets:
        try:
            zip_path = process_board(kicad_cli, name, BOARDS[name], base_dir)
            results[name] = zip_path
        except RuntimeError as e:
            print(f"\nERROR processing {name}: {e}")
            results[name] = None

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
