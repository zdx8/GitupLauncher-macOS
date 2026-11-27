//! Background workers: GitHub fetches, downloads, installs and self-update.

use crate::cache;
use crate::catalog::{self, repos};
use crate::github;
use crate::icon;
use crate::install;
use crate::self_update;
use crate::state::{self, JobPhase, Msg, State};
use crate::version;
use crate::widgets::clipboard_text;
use crate::{Batch, Launcher, Page, Picker};
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

impl Launcher {
    /// Whether this repository is subscribed, and a toggle that keeps the
    /// setting file in step.
    pub(crate) fn toggle_subscription(&mut self, idx: usize) {
        let id = repos()[idx].id;
        let on = !self.settings.disabled.iter().any(|d| d == id);
        if on {
            self.settings.disabled.push(id.to_string());
            self.state.subscribed[idx] = false;
            self.state.status = format!("已取消订阅 {}", repos()[idx].name);
        } else {
            self.settings.disabled.retain(|d| d != id);
            self.state.subscribed[idx] = true;
            self.state.status = format!("已订阅 {}", repos()[idx].name);
        }
        state::save_settings(&self.settings);
    }

    /// Effective `.app` bundle name for repository `idx`.
    pub(crate) fn bundle_name(&self, idx: usize) -> String {
        let repo = repos()[idx];
        state::effective_bundle(repo.slug, repo.bundle, &self.settings.bundle_overrides).to_string()
    }

    /// When an installed `.app` was found under a different folder name than
    /// the subscription expects, remember that stem so later opens/icons hit
    /// the same path without scanning again. Also collapses duplicate
    /// installs that share one `CFBundleIdentifier` but different folder names.
    pub(crate) fn discover_bundle_overrides(&mut self) -> bool {
        let mut dirty = false;
        let install_dir = self.settings.install_dir.clone();
        for repo in repos() {
            let bundle = state::effective_bundle(
                repo.slug,
                repo.bundle,
                &self.settings.bundle_overrides,
            );
            if let Some(stem) = install::discover_bundle_stem(bundle, &install_dir, repo.slug) {
                let needs = self
                    .settings
                    .bundle_overrides
                    .get(repo.slug)
                    .map(|s| s != &stem)
                    .unwrap_or(true);
                if needs {
                    self.settings
                        .bundle_overrides
                        .insert(repo.slug.to_string(), stem);
                    dirty = true;
                }
            }
        }
        // Collapse rename leftovers (same CFBundleIdentifier, two folder names).
        // Keeps the higher version — e.g. Snow Shot 1.6.5 over snow-shot 1.6.3.
        let removed = install::dedupe_same_identifier(&install_dir, None);
        if !removed.is_empty() {
            dirty = true;
            let names: Vec<String> = removed
                .iter()
                .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
                .collect();
            if !names.is_empty() {
                self.state.status = format!("已清理重复安装：{}", names.join("、"));
            }
        }
        if dirty {
            state::save_settings(&self.settings);
        }
        dirty
    }

    /// Read every installed app's own icon off the UI thread (with disk PNG cache).
    pub(crate) fn spawn_icon_load(&mut self) {
        let _ = self.discover_bundle_overrides();
        self.state
            .rescan_installed(&self.settings.install_dir, &self.settings.bundle_overrides);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let install_dir = self.settings.install_dir.clone();
        let bundles: Vec<(usize, String, String)> = repos()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.state.is_installed(*i))
            .map(|(i, repo)| {
                let b = state::effective_bundle(
                    repo.slug,
                    repo.bundle,
                    &self.settings.bundle_overrides,
                )
                .to_string();
                (i, b, repo.slug.to_string())
            })
            .collect();
        std::thread::spawn(move || {
            for (idx, bundle, slug) in bundles {
                let Some(bytes) =
                    install::bundle_icon_png_for(&bundle, &install_dir, Some(&slug))
                else {
                    continue;
                };
                if let Ok((width, height, mut rgba)) = icon::decode_png_rgba(&bytes) {
                    icon::clear_white_plate(width, height, &mut rgba);
                    let uv = icon::artwork_uv(width, height, &rgba);
                    let _ = tx.send(Msg::Icon {
                        idx,
                        width,
                        height,
                        uv,
                        rgba,
                    });
                    ctx.request_repaint();
                }
            }
        });
    }

    /// Ask GitHub about every subscribed repository, using ETag conditional requests.
    pub(crate) fn spawn_meta_fetch_all(&mut self) {
        self.state.set_all_fetching();
        self.state.status = "正在检查更新…".to_string();
        let include_pre = self.settings.include_pre;
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        for idx in 0..repos().len() {
            if !self.state.subscribed[idx] {
                continue;
            }
            let tx = self.tx.clone();
            let ctx = self.ctx.clone();
            let slug = repos()[idx].slug.to_string();
            let cached_desc = self.descriptions.contains_key(&slug);
            let etag = if self.releases.contains_key(&slug) {
                self.etags.get(&slug).cloned()
            } else {
                None
            };
            let tok = token.clone();
            std::thread::spawn(move || {
                let result = github::fetch_latest_release(
                    &slug,
                    include_pre,
                    etag.as_deref(),
                    tok.as_deref(),
                );
                let _ = tx.send(Msg::Meta { idx, result });
                if !cached_desc {
                    if let Some(text) = github::fetch_repo_description(&slug, tok.as_deref()) {
                        let _ = tx.send(Msg::Description { slug, text });
                    }
                }
                ctx.request_repaint();
            });
        }
    }

    /// Rebuild every per-row table after the live list has changed shape.
    pub(crate) fn rebuild_repos(&mut self) {
        for repo in self.state.repos.iter() {
            if let Some(job) = &repo.job {
                job.cancel.store(true, Ordering::SeqCst);
            }
        }
        let page = match self.page {
            Page::Detail(_) => Page::Grid,
            other => other,
        };
        let mut fresh = State::new();
        fresh.layout = self.state.layout;
        fresh.sort = self.state.sort;
        for (i, repo) in repos().iter().enumerate() {
            fresh.subscribed[i] = !self.settings.disabled.iter().any(|d| d == repo.id);
        }
        fresh.rescan_installed(&self.settings.install_dir, &self.settings.bundle_overrides);
        self.state = fresh;
        self.icons = repos().iter().map(|_| None).collect();
        self.icon_uv = repos().iter().map(|_| (0.0, 0.0, 1.0, 1.0)).collect();
        self.clamp_cache.clear();
        self.page = page;
        self.spawn_meta_fetch_all();
        self.spawn_icon_load();
    }

    /// Move the installed app to the Bin. The subscription stays.
    pub(crate) fn uninstall(&mut self, idx: usize) {
        if let Some(job) = self.state.repos.get(idx).and_then(|r| r.job.as_ref()) {
            job.cancel.store(true, Ordering::SeqCst);
        }
        let name = repos()[idx].name;
        let bundle = self.bundle_name(idx);
        match install::uninstall_app(&bundle, &self.settings.install_dir) {
            Ok(()) => {
                self.state
                    .rescan_installed(&self.settings.install_dir, &self.settings.bundle_overrides);
                self.state.status = format!("已卸载 {name}（订阅保留）");
            }
            Err(e) => self.state.status = format!("卸载 {name} 失败: {e}"),
        }
    }

    /// Drop the subscription only: whatever is installed stays installed.
    pub(crate) fn delete_subscription(&mut self, idx: usize) {
        let name = repos()[idx].name;
        let builtin = catalog::is_builtin(repos()[idx].slug);
        if builtin {
            self.toggle_subscription(idx);
        } else {
            self.remove_repository(idx);
        }
        self.state.status = format!("已删除订阅 {name}（已安装的 {name}.app 保留）");
    }

    /// Remove a repository the person added. Built-ins are only ever switched off.
    pub(crate) fn remove_repository(&mut self, idx: usize) {
        let list = repos();
        if idx >= list.len() || catalog::is_builtin(list[idx].slug) {
            return;
        }
        if self.state.busy(idx) {
            self.state.status = "该仓库正在下载，请稍后再移除".to_string();
            return;
        }
        let slug = list[idx].slug.to_string();
        let id = list[idx].id.to_string();
        catalog::remove_custom(&slug);
        self.settings.custom.retain(|s| s != &slug);
        self.settings.disabled.retain(|d| d != &id);
        self.settings.bundle_overrides.remove(&slug);
        state::save_settings(&self.settings);
        self.rebuild_repos();
        self.state.status = format!("已移除订阅 {slug}");
    }

    /// Start updating every subscribed app that currently has an update waiting.
    pub(crate) fn update_all(&mut self) {
        let targets: Vec<usize> = (0..repos().len())
            .filter(|&i| {
                self.state.subscribed[i] && self.state.has_update(i) && !self.state.busy(i)
            })
            .collect();
        if targets.is_empty() {
            self.state.status = "所有已安装应用均为最新版本".to_string();
            return;
        }
        let count = targets.len();
        for idx in targets {
            self.start_job(idx);
        }
        if count > 1 {
            self.state.status = format!("正在下载 {count} 个应用的更新…");
        }
    }

    /// Take whatever the worker threads have said since the last frame.
    pub(crate) fn handle_messages(&mut self) {
        let mut done_seen = false;
        let mut releases_dirty = false;
        let mut etags_dirty = false;
        let mut desc_dirty = false;
        let mut settings_dirty = false;
        while let Ok(msg) = self.rx.try_recv() {
            // A list resize may have left an old answer on the way; it
            // must not land on whatever row happens to sit at that index.
            let idx_ok = |idx: usize, repos: &Vec<state::RepoState>| idx < repos.len();
            match msg {
                Msg::Meta { idx, result } if idx_ok(idx, &self.state.repos) => {
                    let slug = repos()[idx].slug.to_string();
                    match result {
                        Ok(github::ReleaseFetch::Fresh { info, etag }) => {
                            self.releases.insert(slug.clone(), github::to_json(&info));
                            releases_dirty = true;
                            if let Some(tag) = etag {
                                self.etags.insert(slug, tag);
                                etags_dirty = true;
                            }
                            self.state.apply_meta(idx, Ok(info));
                        }
                        Ok(github::ReleaseFetch::NotModified) => {
                            if let Some(rel) = self.releases.get(&slug).and_then(github::from_json)
                            {
                                self.state.apply_meta(idx, Ok(rel));
                            } else {
                                self.etags.remove(&slug);
                                etags_dirty = true;
                            }
                        }
                        Err(e) => match self.releases.get(&slug).and_then(github::from_json) {
                            Some(rel) => {
                                self.state.apply_meta(idx, Ok(rel));
                                self.state.status =
                                    "GitHub 暂时无法访问，显示的是上次读取的版本信息".to_string();
                            }
                            None => self.state.apply_meta(idx, Err(e)),
                        },
                    }
                }
                Msg::Progress { idx, done, total } if idx_ok(idx, &self.state.repos) => {
                    self.state.apply_progress(idx, done, total)
                }
                Msg::Installing { idx } if idx_ok(idx, &self.state.repos) => {
                    self.state.apply_installing(idx)
                }
                Msg::Done { idx, bundle, result } if idx_ok(idx, &self.state.repos) => {
                    let name = repos()[idx].name;
                    match result {
                        Ok(ver) => {
                            if let Some(actual) = bundle {
                                if actual != repos()[idx].bundle {
                                    self.settings
                                        .bundle_overrides
                                        .insert(repos()[idx].slug.to_string(), actual);
                                    settings_dirty = true;
                                }
                            }
                            let was_installed = self.state.repos[idx].installed.is_some();
                            if !ver.is_empty() {
                                self.state.repos[idx].installed = Some(ver);
                            }
                            self.state.repos[idx].job = None;
                            let word = if was_installed { "已更新" } else { "已安装" };
                            let shown = self.state.repos[idx]
                                .installed
                                .clone()
                                .unwrap_or_default();
                            self.state.status = format!("{word} {name} {shown}");
                        }
                        Err(e) => {
                            self.state.repos[idx].job = None;
                            self.state.status = if e == "cancelled" {
                                format!("已取消下载 {name}")
                            } else {
                                format!("安装失败: {e}")
                            };
                        }
                    }
                    done_seen = true;
                }
                Msg::Icon {
                    idx,
                    width,
                    height,
                    uv,
                    rgba,
                } if idx < self.icons.len() => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &rgba,
                    );
                    let texture = self.ctx.load_texture(
                        format!("app-icon-{}", repos()[idx].id),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.icons[idx] = Some(texture);
                    self.icon_uv[idx] = uv;
                }
                Msg::Versions { idx, result } => {
                    if let Some(picker) = self.picker.as_mut() {
                        if picker.idx == idx {
                            picker.loading = false;
                            match result {
                                Ok(list) => {
                                    picker.list = list;
                                    picker.chosen = 0;
                                }
                                Err(e) => picker.error = Some(e),
                            }
                        }
                    }
                }
                Msg::Description { slug, text } => {
                    self.clamp_cache.retain(|(s, _, _), _| s != &slug);
                    self.descriptions.insert(slug, text);
                    desc_dirty = true;
                }
                Msg::SelfUpdateChecked(res) => match res {
                    Ok(github::ReleaseFetch::Fresh { info, .. }) => {
                        let cur = version::parse_version(self_update::current_version());
                        let latest = version::parse_version(&info.tag);
                        if matches!((cur, latest), (Some(a), Some(b)) if a < b)
                            && info.asset.is_some()
                        {
                            let tag = version::clean_tag(&info.tag).to_uppercase();
                            self.self_update = self_update::SelfUpdateState::Available(info);
                            if self.settings.self_auto_update
                                && self_update::current_app_bundle().is_some()
                            {
                                self.start_self_update_download();
                            } else {
                                self.state.status =
                                    format!("发现 Gitup Launcher 新版本 V{tag}，点击右上角即可更新");
                            }
                        } else {
                            self.self_update = self_update::SelfUpdateState::UpToDate;
                        }
                    }
                    Ok(github::ReleaseFetch::NotModified) => {
                        self.self_update = self_update::SelfUpdateState::UpToDate;
                    }
                    Err(e) => {
                        self.self_update = self_update::SelfUpdateState::Error(e);
                    }
                },
                Msg::SelfUpdateProgress { done, total } => {
                    if let self_update::SelfUpdateState::Downloading {
                        done: d,
                        total: t,
                        ..
                    } = &mut self.self_update
                    {
                        *d = done;
                        *t = total;
                    }
                }
                Msg::SelfUpdateStaging => {
                    if matches!(self.self_update, self_update::SelfUpdateState::Downloading { .. }) {
                        self.self_update = self_update::SelfUpdateState::Staging;
                    }
                }
                Msg::SelfUpdateReady {
                    version,
                    staged_app,
                    staging_dir,
                } => {
                    self.state.status = format!(
                        "Gitup Launcher V{} 已就绪，点击右上角即可重启更新",
                        version.to_uppercase()
                    );
                    self.self_update = self_update::SelfUpdateState::Ready {
                        version,
                        staged_app,
                        staging_dir,
                    };
                }
                Msg::SelfUpdateError(e) => {
                    if e == "cancelled" {
                        self.state.status = "已取消启动器更新".to_string();
                        self.self_update = self_update::SelfUpdateState::Idle;
                    } else {
                        self.state.status = format!("启动器更新失败: {e}");
                        self.self_update = self_update::SelfUpdateState::Error(e);
                    }
                }
                // Anything arriving for a row that no longer exists is
                // simply dropped.
                _ => {}
            }
        }
        if releases_dirty {
            cache::save_releases(&self.releases);
        }
        if etags_dirty {
            cache::save_etags(&self.etags);
        }
        if desc_dirty {
            cache::save_descriptions(&self.descriptions);
        }
        if settings_dirty {
            state::save_settings(&self.settings);
        }
        if done_seen {
            self.state
                .rescan_installed(&self.settings.install_dir, &self.settings.bundle_overrides);
            // A freshly installed app can now show its own icon.
            self.spawn_icon_load();
        }
    }

    /// Check GitHub Releases for a newer version of Gitup Launcher itself.
    pub(crate) fn check_self_update(&mut self) {
        if self.self_update.is_busy() {
            return;
        }
        self.self_update = self_update::SelfUpdateState::Checking;
        let slug = self_update::self_repo_slug();
        let include_pre = self.settings.include_pre;
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let res = github::fetch_latest_release(&slug, include_pre, None, token.as_deref());
            let _ = tx.send(Msg::SelfUpdateChecked(res));
            ctx.request_repaint();
        });
    }

    /// Download and stage the available self-update release off the UI thread.
    pub(crate) fn start_self_update_download(&mut self) {
        let rel = match &self.self_update {
            self_update::SelfUpdateState::Available(r) => r.clone(),
            _ => return,
        };
        let Some(asset) = rel.asset.clone() else {
            self.self_update =
                self_update::SelfUpdateState::Error("该版本未提供 macOS 安装包".to_string());
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let version = version::clean_tag(&rel.tag);
        self.self_update = self_update::SelfUpdateState::Downloading {
            done: 0,
            total: asset.size,
            cancel: cancel.clone(),
        };
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join("gitup-launcher/self-update");
            let _ = std::fs::create_dir_all(&dir);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let file = dir.join(format!("self-{stamp}-{}", asset.name));
            let mut urls = vec![asset.url.clone()];
            if asset.browser_url != asset.url {
                urls.push(asset.browser_url.clone());
            }
            let dl = github::download_file(
                &urls,
                &file,
                token.as_deref(),
                &cancel,
                |done, total| {
                    let _ = tx.send(Msg::SelfUpdateProgress { done, total });
                    ctx.request_repaint();
                },
            );
            let outcome = dl.and_then(|_| {
                let _ = tx.send(Msg::SelfUpdateStaging);
                ctx.request_repaint();
                self_update::stage_update(&file, asset.kind)
            });
            let _ = std::fs::remove_file(&file);
            match outcome {
                Ok((staged_app, staging_dir)) => {
                    let _ = tx.send(Msg::SelfUpdateReady {
                        version,
                        staged_app,
                        staging_dir,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Msg::SelfUpdateError(e));
                }
            }
            ctx.request_repaint();
        });
    }

    /// Swap the staged `.app` into place via a detached helper and relaunch.
    pub(crate) fn apply_self_update_and_relaunch(&mut self) {
        let (staged_app, staging_dir) = match &self.self_update {
            self_update::SelfUpdateState::Ready {
                staged_app,
                staging_dir,
                ..
            } => (staged_app.clone(), staging_dir.clone()),
            _ => return,
        };
        match self_update::apply_and_relaunch(
            &staged_app,
            &staging_dir,
            &self.settings.install_dir,
        ) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                self.state.status = format!("重启更新失败: {e}");
                self.self_update = self_update::SelfUpdateState::Error(e);
            }
        }
    }

    /// Download the newest release and install it into the configured directory.
    pub(crate) fn start_job(&mut self, idx: usize) {
        if self.state.busy(idx) {
            return;
        }
        let Some(rel) = self.state.release(idx) else {
            if self.state.is_fetching(idx) {
                self.state.status = "正在获取版本信息，请稍候…".to_string();
            } else {
                self.state.status = format!(
                    "未能获取 {} 的版本信息：{}",
                    repos()[idx].name,
                    self.state.fetch_error(idx).unwrap_or("没有可用的安装包")
                );
            }
            return;
        };
        let Some(asset) = rel.asset.clone() else {
            self.state.status =
                format!("{} 的最新发布里没有 macOS 安装包", repos()[idx].name);
            return;
        };
        self.start_download(idx, asset);
    }

    /// Download and install one particular asset. The version chooser uses
    /// this to install a release that is not the newest.
    pub(crate) fn start_download(&mut self, idx: usize, asset: github::MacAsset) {
        if self.state.busy(idx) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.state.repos[idx].job = Some(state::Job {
            progress: 0.0,
            speed: 0.0,
            last: None,
            done: 0,
            total: 0,
            phase: JobPhase::Downloading,
            cancel: cancel.clone(),
        });
        self.state.status = format!("正在下载 {}…", repos()[idx].name);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let bundle = self.bundle_name(idx);
        let install_dir = self.settings.install_dir.clone();
        let preserve_name = !catalog::is_builtin(repos()[idx].slug);
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join("gitup-launcher/download");
            let _ = std::fs::create_dir_all(&dir);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let file = dir.join(format!("{idx}-{stamp}-{}", asset.name));
            let _ = std::fs::remove_file(&file);
            // The API's link first, the browser link as a second chance.
            let mut urls = vec![asset.url.clone()];
            if asset.browser_url != asset.url {
                urls.push(asset.browser_url.clone());
            }
            let installed = github::download_file(
                &urls,
                &file,
                token.as_deref(),
                &cancel,
                |done, total| {
                    let _ = tx.send(Msg::Progress { idx, done, total });
                    ctx.request_repaint();
                },
            )
            .and_then(|_| {
                let _ = tx.send(Msg::Installing { idx });
                ctx.request_repaint();
                install::install_asset(&file, asset.kind, &bundle, &install_dir, preserve_name)
            });
            let _ = std::fs::remove_file(&file);
            let (actual_bundle, result) = match installed {
                Ok(actual) => {
                    let ver = install::installed_version(&actual, &install_dir).unwrap_or_default();
                    (Some(actual), Ok(ver))
                }
                Err(e) => (None, Err(e)),
            };
            let _ = tx.send(Msg::Done {
                idx,
                bundle: actual_bundle,
                result,
            });
            ctx.request_repaint();
        });
    }

    /// Open the installed app.
    pub(crate) fn open_app(&mut self, idx: usize) {
        let bundle = self.bundle_name(idx);
        match install::open_app(&bundle, &self.settings.install_dir) {
            Ok(()) => self.state.status = format!("已打开 {}", repos()[idx].name),
            Err(e) => self.state.status = format!("打开失败: {e}"),
        }
    }

    /// Show the repository's page in the browser.
    pub(crate) fn open_repo_page(&mut self, idx: usize) {
        let url = format!("https://github.com/{}", repos()[idx].slug);
        let _ = std::process::Command::new("open").arg(&url).spawn();
        self.state.status = format!("已在浏览器打开 {}", repos()[idx].slug);
    }

    /// Copy the repository's address.

    /// What the big button on a card does.
    pub(crate) fn act(&mut self, idx: usize) {
        if self.state.busy(idx) {
            // Clicking a running download cancels it; an install that is
            // already in place cannot be stopped halfway.
            let cancel = self.state.repos[idx]
                .job
                .as_ref()
                .filter(|j| j.phase == JobPhase::Downloading)
                .map(|j| j.cancel.clone());
            if let Some(cancel) = cancel {
                cancel.store(true, Ordering::SeqCst);
                self.state.status = format!("正在取消下载 {}…", repos()[idx].name);
            }
        } else if self.state.has_update(idx) {
            self.start_job(idx);
        } else if self.state.is_installed(idx) {
            self.open_app(idx);
        } else {
            self.start_job(idx);
        }
    }

    /// Open the version chooser for one app, asking GitHub for its whole
    /// release list on a worker thread.
    pub(crate) fn open_versions(&mut self, idx: usize) {
        self.menu_for = None;
        self.picker = Some(Picker {
            idx,
            loading: true,
            error: None,
            list: Vec::new(),
            chosen: 0,
            open: false,
        });
        let slug = repos()[idx].slug.to_string();
        let include_pre = self.settings.include_pre;
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = github::fetch_release_list(&slug, include_pre, token.as_deref());
            let _ = tx.send(Msg::Versions { idx, result });
            ctx.request_repaint();
        });
    }

    /// Open the importer. Whatever is on the clipboard is put in it
    /// straight away — its first address in the one-line box, the whole
    /// text in the list box — so something can be added without typing.
    pub(crate) fn open_batch(&mut self) {
        let clip = clipboard_text().unwrap_or_default();
        let first = clip
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or_default()
            .to_string();
        self.batch = Some(Batch {
            single: first,
            text: clip,
            report: None,
            expanded: false,
            focus: true,
        });
    }

    /// Read every address in the box and subscribe to all of them at once.
    /// It answers how many were added, how many were already there and how
    /// many lines could not be read, so the caller knows whether anything
    /// happened at all.
    pub(crate) fn import_repositories(&mut self, text: &str) -> (usize, usize, usize) {
        let mut added = 0usize;
        let mut restored = 0usize;
        let mut known = 0usize;
        let mut bad: Vec<String> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // An address that is already in the list but switched off is
            // switched back on rather than refused — this is where a
            // subscription deleted earlier is added again. The switched-off
            // list holds repository ids, not slugs.
            if let Some(idx) = catalog::lookup(line) {
                let id = repos()[idx].id.to_string();
                if self.settings.disabled.iter().any(|d| d == &id) {
                    self.settings.disabled.retain(|d| d != &id);
                    self.state.subscribed[idx] = true;
                    restored += 1;
                } else {
                    known += 1;
                }
                continue;
            }
            match catalog::add_custom(line) {
                Ok(idx) => {
                    let id = repos()[idx].id.to_string();
                    let slug = repos()[idx].slug.to_string();
                    if !self.settings.custom.iter().any(|s| s == &slug) {
                        self.settings.custom.push(slug);
                    }
                    self.settings.disabled.retain(|d| d != &id);
                    added += 1;
                }
                Err(e) => {
                    if e.contains("已经在订阅") {
                        known += 1;
                    } else {
                        bad.push(format!("{line} —— {e}"));
                    }
                }
            }
        }
        state::save_settings(&self.settings);
        if added > 0 || restored > 0 {
            self.rebuild_repos();
            self.pending_scroll_end = true;
        }
        let mut parts: Vec<String> = Vec::new();
        if added > 0 {
            parts.push(format!("新增 {added} 个"));
        }
        if restored > 0 {
            parts.push(format!("恢复订阅 {restored} 个"));
        }
        if known > 0 {
            parts.push(format!("{known} 个此前已经订阅"));
        }
        if !bad.is_empty() {
            parts.push(format!("{} 行无法识别", bad.len()));
        }
        let report = if parts.is_empty() {
            "没有可添加的地址".to_string()
        } else {
            format!("已导入：{}", parts.join("，"))
        };
        self.state.status = report.clone();
        if let Some(batch) = self.batch.as_mut() {
            batch.report = Some(if bad.is_empty() {
                report
            } else {
                format!("{report}\n\n{}", bad.join("\n"))
            });
        }
        (added + restored, known, bad.len())
    }
}
