# Gitup Launcher

A launcher that looks after the macOS apps a handful of GitHub
repositories publish on Releases: it sees what is installed, fetches
each repository's newest release, and can install, update or open any
of them from one window.

## Open it

Double-click **Gitup Launcher.app** in this folder.

## The repositories it watches

- zdx8/NetScope-macOS · zdx8/iTelly-macOS · zdx8/MediaCurator-macOS
- zdx8/SyncEngine-macOS · zdx8/DataCopier-macOS · zdx8/FTPServer-macOS
- hety0211/lumen-raw · JamesQiu2005/SpektraLab

## What it does

The window follows the Mac's own light or dark setting, so it looks at
home whichever one you use. The layout is anchored to the window's live
edges, so it holds together at any size, and the grid scrolls for real
(wheel, thumb drag, or clicking the track) when the cards outgrow the
window.

- **Top bar** – the GITUP wordmark, the suite's name, and a cog at the
  far right. The cog opens a menu: check for updates now, rescan the
  installed apps, or read a line about the launcher itself.
- **Toolbar** – a **排序** menu (by name or by newest release), a
  **搜索应用** box that narrows the list as you type, and a switch
  between the card grid and a plain list.
- **The grid** – each app is one card: its version in colour (amber when
  a newer release is waiting), a GITHUB link to its page, its tile, its
  name, what it is for, when its newest release was published, and one
  big button that says **打开** when the app is already here, **安装**
  when it is not, and **更新** when a newer release is waiting.
- **The little "…" button** – opens a small menu with the other things
  you can do with that app: open it, install it again, or copy its link.
- **The footer** – what the launcher is doing on the left ("正在检查更
  新…"), and a row of links on the right.

Versions, dates and download links come live from the GitHub Releases
API on worker threads. What is installed is read from each app bundle's
own Info.plist. Installing downloads the release's macOS asset
(preferring arm64, then universal; zip or dmg), unpacks it with ditto
or mounts it with hdiutil, and replaces the copy in /Applications —
moving the old one aside first and putting it back if anything fails.
Clicking a running download cancels it.

Set `GITHUB_TOKEN` in the environment if the API's anonymous rate limit
(60 requests an hour) ever gets in the way.

## Building it again

Run `./bundle.sh` in this folder. It builds the program and refreshes
the app bundle next to it.

Cargo's usual download folder is not writable here, so the build needs to
be told where to keep it:

```sh
export CARGO_HOME="$PWD/.cargo-home"
cargo build --release
```

## Looking at the other screens

The program can open straight onto one of the other views, which is
handy for taking pictures of them:

```sh
./target/release/gitup-launcher --demo list
./target/release/gitup-launcher --demo menu
./target/release/gitup-launcher --demo sort
./target/release/gitup-launcher --demo search
```

Setting `GITUP_THEME=light` (or `dark`) shows the other theme.

Adding `--screenshot some-file.png` saves a picture of the window and
quits.
