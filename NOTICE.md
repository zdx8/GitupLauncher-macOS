# Third-party notices

The launcher itself is MIT licensed (see LICENSE). Two things it ships
are somebody else's work.

## Typefaces

`assets/fonts/` holds three variable typefaces, each under the SIL Open
Font License, Version 1.1. They are bundled so the window looks the same
on every Mac, whether or not the fonts are installed.

| File | Family | Upstream | License |
| --- | --- | --- | --- |
| `Archivo-Variable.ttf` | Archivo | https://github.com/Omnibus-Type/Archivo | SIL OFL 1.1 |
| `GeistMono-Variable.ttf` | Geist Mono | https://github.com/vercel/geist-font | SIL OFL 1.1 |
| `Inter-Variable.ttf` | Inter | https://github.com/rsms/inter | SIL OFL 1.1 |

The full text of the SIL Open Font License 1.1 is at
https://openfontlicense.org/open-font-license-official-text/ .

## Rust crates

The program is built on [egui](https://github.com/emilk/egui) (MIT or
Apache-2.0) together with `ureq`, `serde_json` and `png`. `Cargo.lock`
lists every crate and its version; each one keeps its own license, and
all of them are permissive.

## The applications it launches

Gitup Launcher only reads the public GitHub Releases of the repositories
listed in `src/catalog.rs`, and downloads the files those repositories
publish. It does not contain any of them, and it is not affiliated with
any of them.
