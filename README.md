# Gitup Launcher

A small macOS launcher for the apps a handful of GitHub repositories
publish on Releases. It shows what is installed, reads each repository's
newest release, and installs, updates or opens any of them from one
window — no browser, no dragging DMGs around.

![The launcher](docs/screenshot.png)

## Install

Download `GitupLauncher-macOS-arm64.dmg` from
[Releases](../../releases), open it, and drag **Gitup Launcher.app**
into `/Applications`.

The build is signed ad-hoc rather than with an Apple Developer ID, so the
first launch needs the usual macOS move: right-click the app and choose
**Open**, or clear the quarantine flag yourself:

```sh
xattr -dr com.apple.quarantine "/Applications/Gitup Launcher.app"
```

Requires macOS 11 or newer on Apple silicon.

## Build it

```sh
./bundle.sh
```

That builds the program and refreshes `Gitup Launcher.app` next to it.
A Rust toolchain (1.85 or newer) and the Xcode command line tools are
enough; there are no other build steps.

## The repositories it watches

- zdx8/NetScope-macOS · zdx8/iTelly-macOS · zdx8/MediaCurator-macOS
- zdx8/SyncEngine-macOS · zdx8/DataCopier-macOS · zdx8/FTPServer-macOS
- hety0211/lumen-raw · JamesQiu2005/SpektraLab

More can be added with the toolbar's **添加订阅** button: paste one
`owner/repo` or a full GitHub link, or press **批量添加** and give it one
per line. Subscriptions can be removed again from the settings page.

## What it does

The window follows the Mac's own light or dark setting. The layout is
anchored to the window's live edges, so it holds at any size, and the
grid scrolls for real (wheel, thumb drag, or clicking the track) when
the cards outgrow the window. Double-clicking the top bar zooms the
window; dragging the background moves it.

- **Toolbar** – **添加订阅** and **刷新订阅** on the left; a sort menu,
  a search box that narrows the list as you type, and a switch between
  the card grid and a plain list on the right.
- **The grid** – one card per app: its tile, name, publisher, the
  release's version and date, one line about what it is for, and a
  single button that reads **打开** when the app is here, **安装** when
  it is not, and **更新** when a newer release is waiting. Apps that are
  not installed yet sit under a 未安装 heading, dimmed.
- **The card menu** – GitHub 仓库, 更新说明, 所有版本, 选择版本 (install
  any earlier release), then 卸载软件 and 删除订阅.
- **One app's page** – the release history, the changelog, and arrows
  that step through the other subscriptions.
- **Settings** – appearance, when to check for updates, where installs
  go, an optional GitHub token, and the list of subscriptions with an ×
  to drop one.

Versions, dates and download links come live from the GitHub Releases
API on worker threads, so the window never waits on the network. What is
installed is read from each app bundle's own Info.plist. Installing
downloads the release's macOS asset (arm64 first, then universal; zip or
dmg), unpacks it with ditto or mounts it with hdiutil, and replaces the
copy in the install folder — moving the old one aside first and putting
it back if anything fails. Clicking a running download cancels it.

## Updating itself

The launcher reads its own releases from `zdx8/GitupLauncher-macOS` too.
When a newer version is published, the top bar offers it: the download
runs in the background, the new bundle is staged next to the old one, and
a small helper swaps it in and reopens the app once the current process
has quit. Nothing is replaced while the app is still running, and the
previous bundle is restored if the copy fails.

## Rate limits

The GitHub API allows 60 anonymous requests an hour, which is only a few
refreshes. A token raises that to 5000: paste one under
**Settings → GitHub Token** (it is stored only in
`~/Library/Application Support/GitupLauncher/settings.json`, readable by
your account alone), or set `GITHUB_TOKEN` before launching.

## Looking at the other screens

The program can open straight onto one of its views, which is handy for
taking pictures of them:

```sh
./target/release/gitup-launcher --demo list
./target/release/gitup-launcher --demo detail
./target/release/gitup-launcher --demo settings
```

Setting `GITUP_THEME=light` (or `dark`) shows the other appearance, and
adding `--screenshot some-file.png` saves a picture of the window and
quits.

## License

MIT — see [LICENSE](LICENSE). The bundled typefaces are under the SIL
Open Font License; see [NOTICE.md](NOTICE.md).
