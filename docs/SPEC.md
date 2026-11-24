# muud-view — Feature Spec

A fast, ad-free image viewer for Windows, written in Rust.

## Clean-room policy

muud-view is written from scratch. The feature list and key bindings below come
only from **publicly published documentation** and everyday use of common image
viewers (for example, Bandisoft's public Honeyview shortcut help page). Key
bindings and feature ideas are functional conventions; no code, binaries, skins,
icons or other assets of any third-party viewer were decompiled, copied or used.

Code that is borrowed from permissive open-source projects must keep its license
notice; see `THIRD_PARTY_NOTICES.md` (generated at release time).

## Goals

1. **Instant** – the window appears immediately and the next/previous image is
   already decoded before you press the key.
2. **Familiar** – the key bindings people already know from Korean image viewers.
3. **Quiet** – no ads, no network access, no telemetry, a single portable `.exe`.
4. **Spec-sheet design** – monochrome, bold grotesk headings, monospaced data,
   hairline borders, dotted rules, outlined pill tags.

## Phase 1 — MVP (v0.1)

### Opening
| Action | Input |
|---|---|
| Open a file passed on the command line / "Open with" | `muud-view.exe <file or folder>` |
| Open file dialog | `Ctrl+O`, `F2` |
| Drag & drop a file or folder onto the window | mouse |
| Close the current file | `F4` |
| Exit | `Esc`, `X`, `Ctrl+W` |

Opening a file loads every supported image in the same folder, sorted in
natural (Explorer-like) order: `img2.jpg` comes before `img10.jpg`.

### Moving between images
| Action | Input |
|---|---|
| Next image | `PgDn`, `→`, `↓`, `Space`, wheel down |
| Previous image | `PgUp`, `←`, `↑`, `Backspace`, `Shift+Space`, wheel up |
| First / last image | `Home` / `End` |
| Jump 10 images | `Ctrl+PgDn` / `Ctrl+PgUp` |

The images just before and after the current one are decoded on background
threads, so moving between them is instant. Decoded images are kept in a cache
with a fixed memory budget.

### View & zoom
| Action | Input |
|---|---|
| Fit to window (default) | `9`, `1` |
| Original size 100% | `0`, `*`, `Ctrl+middle click` |
| Fit to width | `8` |
| Zoom in / out | `+` / `-`, `Ctrl+wheel` (zooms at the cursor) |
| Pan | drag with left mouse button; arrow keys while zoomed |
| Fullscreen | `Enter`, `Alt+Enter`, `F11`, `N`, middle click |
| Maximize / restore | `M`, double click |
| Always on top | `Ctrl+A` |
| Smooth / pixel-exact scaling | `I` / `U` |
| Canvas background (ink / paper / checker) | `B` |

EXIF orientation is applied automatically.

### Transform (view only — the file is never modified)
| Action | Input |
|---|---|
| Rotate 90° clockwise / counter-clockwise | `Ctrl+R`, `Alt+→` / `Ctrl+L`, `Alt+←` |
| Rotate 180° | `Alt+↓` |
| Reset rotation | `Alt+↑` |
| Flip horizontal / vertical | `Ctrl+M` / `Ctrl+F` |

### File actions
| Action | Input |
|---|---|
| Info panel (file, image, camera, exposure, GPS, histogram) | `Tab` |
| Copy image to clipboard | `Ctrl+C` |
| Copy file path | `Ctrl+Alt+Shift+C` |
| Copy EXIF summary | `Ctrl+Shift+C` |
| Show in Explorer | `Ctrl+Enter` |
| Move to Recycle Bin | `Delete` |
| Delete permanently (asks first) | `Shift+Delete` |
| Slideshow every N seconds / stop | `Ctrl+1`…`Ctrl+9` / `Ctrl+0` |
| Shortcut help | `F1` |

### Formats
JPEG, PNG, GIF (animated), WebP (animated), BMP, TIFF, TGA, ICO, PNM, QOI,
DDS, HDR, OpenEXR — all decoded by pure-Rust decoders.

### Platform
- Windows 10/11 x64, a single portable `muud-view.exe` with the C runtime
  linked statically, so nothing else needs to be installed.
- OpenGL renderer (works in VMs and over Remote Desktop).
- Settings (window size, background, smoothing, info panel) persist between runs.

## Phase 2

- Zip/CBZ archives (including CP949 Korean file names), next/previous folder `]` `[`
- Double-page and webtoon (vertical strip) modes
- Register as the default viewer / Explorer "Open with" entry from the app
- AVIF, HEIC (licensing to be reviewed — libheif is LGPL), camera RAW previews
- Very large images (> 16k px) shown as tiles without downscaling
- Customizable key bindings, Korean/English UI

## Non-goals

Image editing, cataloging, cloud sync, anything that needs the network.
