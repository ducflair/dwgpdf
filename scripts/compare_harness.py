#!/usr/bin/env python3
"""
Comparison Harness for CAD-to-PDF Conversion
Validates visual fidelity between dwgpdf output and reference renderer (Autodesk Viewer).
"""

import argparse
import json
import math
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from PIL import Image

WORKSPACE_DIR = Path(__file__).resolve().parent.parent
TARGET_BIN = Path(os.environ.get("CARGO_TARGET_DIR", WORKSPACE_DIR / "target")) / "release" / "dwgpdf"
TEMP_DIR = Path(tempfile.gettempdir())
TESTS_OUTPUT_DIR = WORKSPACE_DIR / "tests" / "tests_output"
REFERENCES_DIR = TESTS_OUTPUT_DIR / "references"


def render_fixture_to_pdf(fixture_path: Path, output_pdf: Path) -> bool:
    """Invokes dwgpdf CLI binary to convert CAD fixture to PDF."""
    if not TARGET_BIN.exists():
        print(f"[ERROR] Binary {TARGET_BIN} not found. Run cargo build --release first.", file=sys.stderr)
        return False

    temp_pdf = TEMP_DIR / f"harness_{fixture_path.stem}.pdf"
    cmd = [str(TARGET_BIN), str(fixture_path), "-o", str(temp_pdf)]
    t0 = time.time()
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    elapsed = time.time() - t0

    if res.returncode != 0 or not temp_pdf.exists():
        print(f"[FAIL] Conversion of {fixture_path.name} failed (exit {res.returncode}):\n{res.stderr}", file=sys.stderr)
        return False

    try:
        output_pdf.parent.mkdir(parents=True, exist_ok=True)
        import shutil
        shutil.copy2(temp_pdf, output_pdf)
    except Exception:
        # If writing to workspace is restricted by sandbox, keep temp_pdf as output
        pass

    print(f"[OK] Rendered {fixture_path.name} -> {temp_pdf.name} in {elapsed:.2f}s ({temp_pdf.stat().st_size} bytes)")
    return True


def rasterize_pdf_pages(pdf_path: Path, output_png: Path, size: tuple[int, int] | None = None) -> list[Path]:
    """Uses pypdf to split pages (if multi-page) and macOS sips to rasterize them."""
    temp_pdf = TEMP_DIR / f"harness_{pdf_path.stem}.pdf"
    target_pdf = temp_pdf if temp_pdf.exists() else pdf_path

    import pypdf
    try:
        reader = pypdf.PdfReader(str(target_pdf))
        num_pages = len(reader.pages)
    except Exception:
        num_pages = 1

    png_paths = []
    if num_pages <= 1:
        temp_png = TEMP_DIR / f"harness_{output_png.stem}.png"
        cmd = ["sips", "-s", "format", "png"]
        if size:
            max_dim = max(size[0], size[1])
            cmd.extend(["-Z", str(max_dim)])
        cmd.extend([str(target_pdf), "--out", str(temp_png)])
        res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        if res.returncode == 0 and temp_png.exists():
            png_paths.append(temp_png)
    else:
        for idx in range(num_pages):
            try:
                writer = pypdf.PdfWriter()
                writer.add_page(reader.pages[idx])
                page_pdf = TEMP_DIR / f"harness_{pdf_path.stem}_p{idx+1}.pdf"
                with open(page_pdf, "wb") as f:
                    writer.write(f)
                page_png = TEMP_DIR / f"harness_{output_png.stem}_p{idx+1}.png"
                cmd = ["sips", "-s", "format", "png"]
                if size:
                    max_dim = max(size[0], size[1])
                    cmd.extend(["-Z", str(max_dim)])
                cmd.extend([str(page_pdf), "--out", str(page_png)])
                res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                if res.returncode == 0 and page_png.exists():
                    png_paths.append(page_png)
            except Exception as e:
                print(f"[WARN] Failed rasterizing page {idx+1}: {e}", file=sys.stderr)

    return png_paths


def compute_image_metrics(img_a: Image.Image, img_b: Image.Image, diff_output: Path | None = None) -> dict:
    """
    Computes visual difference metrics:
    - MSE (Mean Squared Error)
    - PSNR (Peak Signal to Noise Ratio)
    - Pixel Match Percentage (within tolerance)
    Generates a visual diff image highlighting discrepancies in bright magenta.
    """
    if diff_output:
        diff_output = Path(diff_output)
    # Normalize dimensions
    w = min(img_a.width, img_b.width)
    h = min(img_a.height, img_b.height)

    a_crop = img_a.crop((0, 0, w, h)).convert("RGB")
    b_crop = img_b.crop((0, 0, w, h)).convert("RGB")

    pixels_a = a_crop.load()
    pixels_b = b_crop.load()

    diff_img = Image.new("RGB", (w, h), (255, 255, 255))
    diff_pixels = diff_img.load()

    total_sq_err = 0.0
    matching_pixels = 0
    total_pixels = w * h
    tolerance = 15  # Per-channel threshold for minor antialiasing differences

    for y in range(h):
        for x in range(w):
            r1, g1, b1 = pixels_a[x, y]
            r2, g2, b2 = pixels_b[x, y]

            dr = r1 - r2
            dg = g1 - g2
            db = b1 - b2

            sq_err = (dr * dr + dg * dg + db * db) / 3.0
            total_sq_err += sq_err

            is_bg_a = r1 >= 210 and g1 >= 210 and b1 >= 210 and abs(r1 - g1) <= 6 and abs(g1 - b1) <= 6
            is_bg_b = r2 >= 135 and g2 >= 135 and b2 >= 135 and abs(r2 - g2) <= 6 and abs(g2 - b2) <= 6

            if (is_bg_a and is_bg_b) or (abs(dr) <= tolerance and abs(dg) <= tolerance and abs(db) <= tolerance):
                matching_pixels += 1
                # Soft grayscale representation of matching background
                gray = int(0.299 * r1 + 0.587 * g1 + 0.114 * b1)
                diff_pixels[x, y] = (int(gray * 0.8 + 51), int(gray * 0.8 + 51), int(gray * 0.8 + 51))
            else:
                # Discrepancy highlighted in high-contrast magenta
                diff_pixels[x, y] = (255, 0, 128)

    mse = total_sq_err / total_pixels
    psnr = 10.0 * math.log10((255.0 * 255.0) / mse) if mse > 1e-6 else float("inf")
    match_pct = (matching_pixels / total_pixels) * 100.0

    if diff_output:
        try:
            diff_output.parent.mkdir(parents=True, exist_ok=True)
            diff_img.save(diff_output)
        except Exception:
            temp_diff = TEMP_DIR / diff_output.name
            diff_img.save(temp_diff)

    return {
        "width": w,
        "height": h,
        "total_pixels": total_pixels,
        "matching_pixels": matching_pixels,
        "match_percentage": round(match_pct, 2),
        "mse": round(mse, 2),
        "psnr_db": round(psnr, 2) if psnr != float("inf") else 999.0,
    }


def compare_fixture(fixture_path: Path, reference_image: Path | None = None) -> dict:
    """Processes a fixture, generates PDF, rasterizes, and compares against reference if available."""
    rel = fixture_path.name
    category = fixture_path.parent.name
    pdf_out = TESTS_OUTPUT_DIR / category / f"{fixture_path.stem}.pdf"
    png_out = TESTS_OUTPUT_DIR / category / f"{fixture_path.stem}.png"
    diff_out = TESTS_OUTPUT_DIR / category / f"{fixture_path.stem}_diff.png"

    ok = render_fixture_to_pdf(fixture_path, pdf_out)
    if not ok:
        return {"fixture": str(fixture_path), "status": "FAILED_CONVERT"}

    ref = reference_image
    if not ref:
        default_ref = REFERENCES_DIR / category / f"{fixture_path.stem}_ref.png"
        if default_ref.exists():
            ref = default_ref
        else:
            ref_dir = WORKSPACE_DIR / "tests" / "fixtures" / "reference_renders"
            import re
            import unicodedata

            stem = fixture_path.stem
            clean = re.sub(r'%\d+', '_', stem)
            clean = re.sub(r'\s*\(\d+\)\s*', '', clean)
            clean = unicodedata.normalize('NFKD', clean).encode('ASCII', 'ignore').decode('utf-8')
            clean = re.sub(r'[^a-zA-Z0-9]+', '_', clean).strip('_').lower()

            # First check exact candidates
            candidates = [
                ref_dir / f"{fixture_path.stem}.png",
                ref_dir / f"{fixture_path.stem.replace(' ', '_')}.png",
                ref_dir / f"{fixture_path.stem.replace('%20', '_').replace(' ', '_')}.png",
            ]
            for c in candidates:
                if c.exists():
                    ref = c
                    break

            # Then check normalized name matching
            if not ref and ref_dir.exists():
                for cand_file in ref_dir.glob("*.png"):
                    c_clean = unicodedata.normalize('NFKD', cand_file.stem).encode('ASCII', 'ignore').decode('utf-8')
                    c_clean = re.sub(r'[^a-zA-Z0-9]+', '_', c_clean).strip('_').lower()
                    if clean == c_clean or clean.startswith(c_clean) or c_clean.startswith(clean):
                        ref = cand_file
                        break

    ref_size = None
    if ref and ref.exists():
        try:
            ref_size = Image.open(ref).size
        except Exception:
            pass

    page_pngs = rasterize_pdf_pages(pdf_out, png_out, ref_size)
    if not page_pngs:
        return {"fixture": str(fixture_path), "status": "FAILED_RASTERIZE"}

    actual_pdf = pdf_out if pdf_out.exists() else (TEMP_DIR / f"harness_{fixture_path.stem}.pdf")

    res = {
        "fixture": rel,
        "category": category,
        "pdf_size": actual_pdf.stat().st_size if actual_pdf.exists() else 0,
        "status": "PASSING",
    }

    if ref and ref.exists():
        img_ref = Image.open(ref).convert("RGB")
        best_metrics = None
        best_diff = None
        best_page_idx = 0

        for p_idx, cand_png in enumerate(page_pngs):
            try:
                img_out = Image.open(cand_png).convert("RGB")
                if img_out.size != img_ref.size:
                    ratio = min(img_ref.width / img_out.width, img_ref.height / img_out.height)
                    if ratio < 0.999 or ratio > 1.001:
                        new_w = max(1, int(img_out.width * ratio))
                        new_h = max(1, int(img_out.height * ratio))
                        img_out = img_out.resize((new_w, new_h), Image.Resampling.LANCZOS)
                    bg_ref = img_ref.getpixel((0, 0))
                    canvas = Image.new("RGB", img_ref.size, bg_ref)
                    off_x = (img_ref.width - img_out.width) // 2
                    off_y = (img_ref.height - img_out.height) // 2
                    canvas.paste(img_out, (off_x, off_y))
                    img_out = canvas

                cand_diff = TEMP_DIR / f"harness_{fixture_path.stem}_diff_p{p_idx+1}.png"
                metrics = compute_image_metrics(img_out, img_ref, cand_diff)

                if best_metrics is None or metrics["match_percentage"] > best_metrics["match_percentage"]:
                    best_metrics = metrics
                    best_diff = cand_diff
                    best_page_idx = p_idx
            except Exception as e:
                print(f"[WARN] Error comparing page {p_idx+1}: {e}", file=sys.stderr)

        if best_metrics:
            res["metrics"] = best_metrics
            res["diff_image"] = str(best_diff)
            res["best_page"] = best_page_idx + 1
            if len(page_pngs) > 1:
                print(f"  Visual match (page {best_page_idx+1}/{len(page_pngs)}): {best_metrics['match_percentage']}% (MSE: {best_metrics['mse']}, PSNR: {best_metrics['psnr_db']}dB)")
            else:
                print(f"  Visual match: {best_metrics['match_percentage']}% (MSE: {best_metrics['mse']}, PSNR: {best_metrics['psnr_db']}dB)")
    else:
        res["metrics"] = None

    return res


def main():
    parser = argparse.ArgumentParser(description="CAD to PDF Visual Comparison Harness")
    parser.add_argument("fixtures", nargs="*", help="Optional specific fixture paths to verify")
    parser.add_argument("--reference", "-r", type=Path, help="Explicit reference image path")
    parser.add_argument("--json", action="store_true", help="Output results in JSON format")
    args = parser.parse_args()

    results = []
    if args.fixtures:
        targets = [Path(f) for f in args.fixtures]
    else:
        root = WORKSPACE_DIR / "tests" / "fixtures"
        targets = sorted(list(root.glob("**/*.dxf")) + list(root.glob("**/*.dwg")) + list(root.glob("**/*.dwf")))

    print(f"=== dwgpdf Comparison Harness (Targets: {len(targets)}) ===")
    for target in targets:
        if target.is_file():
            res = compare_fixture(target, args.reference)
            results.append(res)

    if args.json:
        print(json.dumps(results, indent=2))
    else:
        passed = sum(1 for r in results if r.get("status") == "PASSING")
        print(f"\nSummary: {passed}/{len(results)} fixtures passed conversion.")


if __name__ == "__main__":
    main()
