"""Decode the same JPEG files with libjpeg-turbo (through Pillow) for
comparison with muud-view's zune-jpeg path.

    python bench/compare_libjpeg_turbo.py IMG_0001.JPG IMG_0002.JPG
    python bench/compare_libjpeg_turbo.py --synthetic out_dir

Then decode the same files with muud-view:

    MUUD_BENCH_JPEG=IMG_0001.JPG cargo test --release -- --ignored --nocapture bench_jpeg

`--synthetic` writes 32.5 MP (6960x4640, R6 Mark III size) photo-like
files in 4:2:0 and 4:2:2 chroma subsampling. Pillow is only a measuring
tool here; nothing of it ships with muud-view.
"""
import statistics
import sys
import time
from pathlib import Path

from PIL import Image, features


def synthetic(out: Path) -> list[Path]:
    import random

    out.mkdir(parents=True, exist_ok=True)
    w, h = 6960, 4640
    random.seed(1)
    # Smooth gradients plus sensor-like noise, built at 1/4 size and
    # upscaled so it is quick to make but not trivially compressible.
    small = Image.effect_noise((w // 4, h // 4), 40).convert("RGB")
    grad = Image.linear_gradient("L").resize((w, h))
    base = Image.merge("RGB", (grad, grad.rotate(90).resize((w, h)), grad.transpose(0)))
    img = Image.blend(base, small.resize((w, h), Image.BICUBIC), 0.45)
    noise = Image.effect_noise((w, h), 18).convert("RGB")
    img = Image.blend(img, noise, 0.12)
    files = []
    for name, sub in (("420", 2), ("422", 1)):
        p = out / f"synthetic-32mp-{name}.jpg"
        img.save(p, quality=95, subsampling=sub)
        files.append(p)
    return files


def bench(path: Path, runs: int = 5) -> None:
    times, scaled = [], []
    for _ in range(runs):
        t = time.perf_counter()
        with Image.open(path) as im:
            im.load()
            size = im.size
        times.append((time.perf_counter() - t) * 1000)
        t = time.perf_counter()
        with Image.open(path) as im:
            # libjpeg-turbo's scaled IDCT (1/4 size), like a screen preview.
            im.draft("RGB", (size[0] // 4, size[1] // 4))
            im.load()
            small = im.size
        scaled.append((time.perf_counter() - t) * 1000)
    mb = path.stat().st_size / 1e6
    print(
        f"{path.name}: {size[0]}x{size[1]} {mb:.1f} MB  "
        f"full {statistics.median(times):.0f} ms  "
        f"1/4 scaled ({small[0]}x{small[1]}) {statistics.median(scaled):.0f} ms"
    )


def main() -> None:
    print("libjpeg-turbo", features.version_feature("libjpeg_turbo"))
    args = sys.argv[1:]
    if args[:1] == ["--synthetic"]:
        files = synthetic(Path(args[1] if len(args) > 1 else "."))
    else:
        files = [Path(a) for a in args]
    for f in files:
        bench(f)


if __name__ == "__main__":
    main()
