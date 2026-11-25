// File types muud view registers with Windows, each with its own Explorer
// icon. Shared with build.rs (through `include!`), which embeds
// `assets/icons/<icon>.ico` under `icon_id`; keep this file free of `use`
// and crate paths. Icons are drawn by packaging/icons/make_icons.py.

/// One registered file type: its ProgID is `muudview.<icon>`.
pub struct FileType {
    /// Icon file stem and ProgID suffix.
    pub icon: &'static str,
    /// Icon resource id in muud-view.exe (shown as `"exe",-id`).
    pub icon_id: u16,
    /// What Explorer calls these files.
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

pub const FILE_TYPES: &[FileType] = &[
    FileType {
        icon: "jpg",
        icon_id: 101,
        name: "JPEG image",
        extensions: &["jpg", "jpeg", "jpe", "jfif"],
    },
    FileType {
        icon: "png",
        icon_id: 102,
        name: "PNG image",
        extensions: &["png", "apng"],
    },
    FileType {
        icon: "gif",
        icon_id: 103,
        name: "GIF image",
        extensions: &["gif"],
    },
    FileType {
        icon: "webp",
        icon_id: 104,
        name: "WebP image",
        extensions: &["webp"],
    },
    FileType {
        icon: "avif",
        icon_id: 105,
        name: "AVIF image",
        extensions: &["avif"],
    },
    FileType {
        icon: "jxl",
        icon_id: 106,
        name: "JPEG XL image",
        extensions: &["jxl"],
    },
    FileType {
        icon: "heic",
        icon_id: 107,
        name: "HEIC image",
        extensions: &["heic", "heif"],
    },
    FileType {
        icon: "tif",
        icon_id: 108,
        name: "TIFF image",
        extensions: &["tif", "tiff"],
    },
    FileType {
        icon: "raw",
        icon_id: 109,
        name: "Camera RAW image",
        extensions: &[
            "cr2", "cr3", "crw", "nef", "nrw", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
        ],
    },
    FileType {
        icon: "psd",
        icon_id: 110,
        name: "Photoshop image",
        extensions: &["psd"],
    },
    FileType {
        icon: "svg",
        icon_id: 111,
        name: "SVG drawing",
        extensions: &["svg"],
    },
    FileType {
        icon: "cbz",
        icon_id: 112,
        name: "Comic book archive",
        extensions: &["cbz"],
    },
    FileType {
        icon: "image",
        icon_id: 113,
        name: "Image",
        extensions: &["bmp", "tga", "ico", "qoi", "dds", "hdr", "exr"],
    },
];
