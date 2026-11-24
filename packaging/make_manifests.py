"""Writes the scoop and winget manifests for a release.

    python packaging/make_manifests.py v0.6.0 SHA256SUMS.txt out_dir

Run by the Release workflow after packaging; the files are attached to the
GitHub release, ready to submit (see docs/PACKAGING.md).
"""
import json
import sys
from pathlib import Path

REPO = "recall2300/muud-view"
ID = "recall2300.muudview"


def main() -> None:
    tag, sums, out = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    version = tag.lstrip("v")
    zip_name = f"muud-view-{tag}-windows-x64.zip"
    hashes = dict(
        reversed(line.split()) for line in sums.read_text().splitlines() if line.strip()
    )
    zip_hash = hashes[zip_name]
    url = f"https://github.com/{REPO}/releases/download/{tag}/{zip_name}"
    out.mkdir(parents=True, exist_ok=True)

    scoop = {
        "version": version,
        "description": "Fast, keyboard-first image viewer for Windows",
        "homepage": f"https://github.com/{REPO}",
        "license": "MIT",
        "architecture": {"64bit": {"url": url, "hash": zip_hash}},
        "bin": "muud-view.exe",
        "shortcuts": [["muud-view.exe", "muud view"]],
        # Portable mode keeps settings next to the exe; scoop keeps that
        # folder across updates.
        "pre_install": "Set-Content \"$dir\\portable.txt\" ''",
        "persist": "muud-view-data",
        "checkver": {"github": f"https://github.com/{REPO}"},
        "autoupdate": {
            "architecture": {
                "64bit": {
                    "url": f"https://github.com/{REPO}/releases/download/v$version/muud-view-v$version-windows-x64.zip",
                    "hash": {"url": f"https://github.com/{REPO}/releases/download/v$version/SHA256SUMS.txt"},
                }
            }
        },
    }
    (out / "muud-view.json").write_text(json.dumps(scoop, indent=4) + "\n")

    common = f"PackageIdentifier: {ID}\nPackageVersion: {version}\n"
    (out / f"{ID}.yaml").write_text(
        common + "DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.6.0\n"
    )
    (out / f"{ID}.installer.yaml").write_text(
        common
        + "InstallerType: zip\n"
        + "NestedInstallerType: portable\n"
        + "NestedInstallerFiles:\n  - RelativeFilePath: muud-view.exe\n    PortableCommandAlias: muud-view\n"
        + "Installers:\n"
        + f"  - Architecture: x64\n    InstallerUrl: {url}\n    InstallerSha256: {zip_hash.upper()}\n"
        + "ManifestType: installer\nManifestVersion: 1.6.0\n"
    )
    (out / f"{ID}.locale.en-US.yaml").write_text(
        common
        + "PackageLocale: en-US\nPublisher: recall2300\nPackageName: muud view\n"
        + f"PackageUrl: https://github.com/{REPO}\nLicense: MIT\n"
        + "ShortDescription: Fast, keyboard-first image viewer for Windows\n"
        + "Tags:\n  - image-viewer\n  - photo\n  - viewer\n"
        + "ManifestType: defaultLocale\nManifestVersion: 1.6.0\n"
    )
    print("\n".join(sorted(p.name for p in out.iterdir())))


if __name__ == "__main__":
    main()
