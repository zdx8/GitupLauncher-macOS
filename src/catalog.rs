//! The GitHub repositories this launcher looks after.
//!
//! Each entry says where the repo lives, what the app bundle inside its
//! release is called once it is installed, and what the app does.
//! Versions, dates and download links are all fetched live from the
//! GitHub Releases API at run time, so nothing here goes stale.

/// One line of the feature list the detail page shows.
pub struct Feature {
    pub title: &'static str,
    pub desc: &'static str,
}

/// One repository the launcher manages.
pub struct RepoInfo {
    /// The id used inside the program.
    pub id: &'static str,
    /// The name shown on the card.
    pub name: &'static str,
    /// The name of the `.app` bundle once it sits in /Applications —
    /// not always the same as the repo or the display name.
    pub bundle: &'static str,
    /// `owner/repo` on GitHub.
    pub slug: &'static str,
    /// The short Chinese line under the name.
    pub tagline: &'static str,
    /// The English description on the card.
    pub summary: &'static str,
    /// The repository's own website, when it has one.
    pub website: Option<&'static str>,
    /// The four cards the detail page lists under 功能特性.
    pub features: &'static [Feature],
    /// The colour of the name's last word and the version.
    pub bright: (u8, u8, u8),
}

/// The built-in repositories, in the order the launcher starts with.
const BUILTIN: &[RepoInfo] = &[
    RepoInfo {
        id: "netscope",
        name: "NetScope",
        bundle: "NetScope",
        slug: "zdx8/NetScope-macOS",
        tagline: "菜单栏上的应用级流量监控",
        summary: "See which app is using your network, right from the menu bar.",
        website: Some("https://zdx8.github.io/NetScope-macOS/"),
        features: &[
            Feature { title: "菜单栏双行速率", desc: "网络图标加上下行两行实时速率，趋势图右键可选。" },
            Feature { title: "按应用统计", desc: "点开就知道刚才到底是谁在偷偷跑流量。" },
            Feature { title: "灵活的刷新间隔", desc: "采样通道随刷新设置同步重建，不闪退、不卡主线程。" },
            Feature { title: "原生轻量", desc: "SwiftUI 原生实现，菜单栏常驻，开箱即用。" },
        ],
        bright: (0x5F, 0xD0, 0xEE),
    },
    RepoInfo {
        id: "itelly",
        name: "iTelly",
        bundle: "iTelly",
        slug: "zdx8/iTelly-macOS",
        tagline: "IPTV 直播播放器",
        summary: "Watch IPTV playlists on your Mac.",
        website: Some("https://zdx8.github.io/iTelly-macOS/"),
        features: &[
            Feature { title: "多播放列表", desc: "M3U / M3U8 订阅与本地文件，收藏、最近播放与搜索。" },
            Feature { title: "内置播放内核", desc: "完整 VLC 运行时随包分发，装完即用，无需另装 VLC。" },
            Feature { title: "断流自动重连", desc: "1s / 2s / 4s 退避重连加 15 秒看门狗，直播不卡死。" },
            Feature { title: "本地媒体库", desc: "影片带进度条与定位，重启后仍保留。" },
        ],
        bright: (0x74, 0xAA, 0xFF),
    },
    RepoInfo {
        id: "mediacurator",
        name: "MediaCurator",
        bundle: "MediaCurator",
        slug: "zdx8/MediaCurator-macOS",
        tagline: "媒体库整理与分类",
        summary: "Curate and tidy your media library.",
        website: Some("https://zdx8.github.io/MediaCurator-macOS/"),
        features: &[
            Feature { title: "三层判重", desc: "精确重复、相似图片与相似视频统一分组，不重不漏。" },
            Feature { title: "计划先行", desc: "所有改动先生成计划，确认后执行，随时整批撤销。" },
            Feature { title: "批量整理", desc: "按拍摄时间、设备与类型的模板重命名并归档。" },
            Feature { title: "全程离线", desc: "不上传任何文件，清理只进系统回收站。" },
        ],
        bright: (0xFF, 0xC1, 0x4D),
    },
    RepoInfo {
        id: "syncengine",
        name: "SyncEngine",
        bundle: "sync-engine",
        slug: "zdx8/SyncEngine-macOS",
        tagline: "基于内容指纹的文件同步",
        summary: "Sync folders by content hash, not timestamps.",
        website: Some("https://zdx8.github.io/SyncEngine-macOS/"),
        features: &[
            Feature { title: "内容指纹", desc: "SHA-256 逐文件哈希判断变更，不依赖时间戳。" },
            Feature { title: "多核并发", desc: "滑动窗口并发哈希，实测 283 → 1179 MB/s。" },
            Feature { title: "三种目标端", desc: "本地目录、SMB 局域网与 WebDAV 服务器。" },
            Feature { title: "凭据入钥匙串", desc: "密码存系统钥匙串，不落配置文件、不进日志。" },
        ],
        bright: (0xB4, 0x94, 0xFF),
    },
    RepoInfo {
        id: "datacopier",
        name: "DataCopier",
        bundle: "DataCopier",
        slug: "zdx8/DataCopier-macOS",
        tagline: "素材批量拷贝与重命名",
        summary: "Copy and rename media in batches.",
        website: Some("https://zdx8.github.io/DataCopier-macOS/"),
        features: &[
            Feature { title: "拷贝带校验", desc: "逐文件 SHA-256 校验，大批量搬运确保数据无损。" },
            Feature { title: "媒体拷贝", desc: "按设备与年月日归档，按拍摄时间重命名。" },
            Feature { title: "PDF 报告", desc: "环形图、明细与校验值自动生成统计报告。" },
            Feature { title: "USB 自动感知", desc: "插入相机卡或 U 盘自动弹出拷贝任务。" },
        ],
        bright: (0x4A, 0xD9, 0x8D),
    },
    RepoInfo {
        id: "ftpserver",
        name: "FTPServer",
        bundle: "FTPServer",
        slug: "zdx8/FTPServer-macOS",
        tagline: "原生 macOS FTP 服务端",
        summary: "A native FTP server for your Mac.",
        website: Some("https://zdx8.github.io/FTPServer-macOS/"),
        features: &[
            Feature { title: "纯系统框架", desc: "Swift 从零实现，零第三方依赖，约 3 MB。" },
            Feature { title: "协议完整", desc: "RFC 959 加 MLSD/MLST 与 EPSV，支持断点续传。" },
            Feature { title: "多账号管理", desc: "每账号独立根目录与四项权限位。" },
            Feature { title: "实时观测", desc: "速率曲线、活动会话与分级日志。" },
        ],
        bright: (0xFF, 0x7A, 0x7A),
    },
    RepoInfo {
        id: "lumenraw",
        name: "LUMEN RAW",
        bundle: "LUMEN RAW",
        slug: "hety0211/lumen-raw",
        tagline: "RAW 照片处理",
        summary: "Your photos. Your pixels. Your machine.",
        website: None,
        features: &[
            Feature { title: "人像面板", desc: "自动检测人脸并编号：磨皮、美白、瘦脸、白牙。" },
            Feature { title: "无损管线", desc: "宽色域 32-bit 浮点处理，不碰原图。" },
            Feature { title: "编辑历史", desc: "裁切、拉直、翻转与自动水平，改动可回溯。" },
            Feature { title: "全新自动算法", desc: "新的自动曝光与色彩算法，一键到位。" },
        ],
        bright: (0xF9, 0xA8, 0xD4),
    },
    RepoInfo {
        id: "spektralab",
        name: "SpektraLab",
        bundle: "SpektraLab",
        slug: "JamesQiu2005/SpektraLab",
        tagline: "胶片风格照片暗房",
        summary: "A photo darkroom with film soul.",
        website: Some("https://jamesqiu2005.github.io/SpektraLab/"),
        features: &[
            Feature { title: "数字中间片", desc: "负片转平坦中性扫描，再选相纸成片。" },
            Feature { title: "胶片风格", desc: "多种相纸风格的照片暗房体验。" },
            Feature { title: "专业调色", desc: "面向调色的分级工具与胶片色彩科学。" },
            Feature { title: "通用二进制", desc: "一份安装包同时支持 Apple Silicon 与 Intel。" },
        ],
        bright: (0xA5, 0xB4, 0xFC),
    },
];

use std::sync::RwLock;

/// The repositories the launcher manages right now: the built-in ones,
/// plus whatever the person has added. Adding or removing only changes
/// this list; the built-in entries themselves never move.
static LIVE: RwLock<Vec<&'static RepoInfo>> = RwLock::new(Vec::new());

/// Fill the live list with the built-in repositories. Called once at
/// start-up, after the settings have been read.
pub fn init_builtin() {
    let mut live = LIVE.write().unwrap();
    if live.is_empty() {
        live.extend(BUILTIN.iter());
    }
}

/// The repositories the launcher manages right now, in display order.
pub fn repos() -> Vec<&'static RepoInfo> {
    LIVE.read().map(|v| v.clone()).unwrap_or_default()
}

/// Whether a slug belongs to one of the built-in repositories.
pub fn is_builtin(slug: &str) -> bool {
    BUILTIN.iter().any(|r| r.slug == slug)
}

/// Everything a pasted repository address can be reduced to `owner/repo`:
/// a full URL, a clone URL, a URL with a deeper path such as `/releases`,
/// trailing slashes, a `.git` suffix, query strings.
fn reduce_to_slug(input: &str) -> String {
    let mut s = input.trim().to_string();
    for lead in [
        "https://www.github.com/",
        "http://www.github.com/",
        "https://github.com/",
        "http://github.com/",
        "https://gitee.com/",
        "git@github.com:",
        "github.com/",
    ] {
        if let Some(rest) = s.strip_prefix(lead) {
            s = rest.to_string();
            break;
        }
    }
    // A pasted address may carry a query or a fragment.
    let cut = s
        .char_indices()
        .find(|(_, c)| *c == '?' || *c == '#')
        .map(|(i, _)| i);
    if let Some(i) = cut {
        s.truncate(i);
    }
    s = s.trim().trim_end_matches('/').to_string();
    if let Some(rest) = s.strip_suffix(".git") {
        s = rest.trim_end_matches('/').to_string();
    }
    // A deeper path (`/tree/main`, `/releases`, `/issues`) is dropped: the
    // first two pieces are the repository itself.
    let parts: Vec<&str> = s.split('/').filter(|p| !p.trim().is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0].trim(), parts[1].trim())
    } else {
        s
    }
}

/// Check that something typed in looks like `owner/repo` and is not
/// already being managed.
fn check_new_slug(slug: &str) -> Result<String, String> {
    if slug.trim().is_empty() {
        return Err("请输入仓库地址，格式如 owner/repo".to_string());
    }
    let slug = reduce_to_slug(slug);
    let Some((owner, repo)) = slug.split_once('/') else {
        return Err("仓库地址应为 owner/repo 或 GitHub 链接".to_string());
    };
    let ok_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    if owner.is_empty() || repo.is_empty() || repo.contains('/')
        || !owner.chars().all(ok_char) || !repo.chars().all(ok_char)
    {
        return Err("仓库地址应为 owner/repo 或 GitHub 链接".to_string());
    }
    if repos()
        .iter()
        .any(|r| r.slug.eq_ignore_ascii_case(&slug))
    {
        return Err("该仓库已经在订阅列表中".to_string());
    }
    Ok(slug)
}

/// Add a repository the person typed in. Its card is built by the same
/// rules the built-in ones follow: the display name is the repo name
/// without a `-macOS` suffix, the app bundle is that same name, and the
/// release data fills in the version, date and download.
pub fn add_custom(slug: &str) -> Result<usize, String> {
    let slug = check_new_slug(slug)?;
    let info: &'static RepoInfo = Box::leak(Box::new(custom_info(&slug)));
    let mut live = LIVE.write().unwrap();
    live.push(info);
    Ok(live.len() - 1)
}

/// Remove a repository the person added. Built-ins stay.
pub fn remove_custom(slug: &str) {
    let mut live = LIVE.write().unwrap();
    live.retain(|r| r.slug != slug);
}

/// Build an entry for a repository that was not compiled in.
fn custom_info(slug: &str) -> RepoInfo {
    let mut parts = slug.splitn(2, '/');
    let owner = parts.next().unwrap_or("github");
    let repo = parts.next().unwrap_or(slug);
    let name = repo
        .trim_end_matches("-macOS")
        .trim_end_matches("-macos")
        .trim_end_matches("-MacOS")
        .to_string();
    // The boxes are leaked for the process's lifetime, which is exactly
    // how long the live list can point at them.
    let leaker = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let id = leaker(slug.to_string());
    let name = leaker(name);
    let bundle = name;
    let slug_static = leaker(slug.to_string());
    let tagline = leaker(format!("由 {} 订阅的 GitHub 仓库", owner));
    let summary = leaker(slug.to_string());
    let (r, g, b) = tile_color(id);
    RepoInfo {
        id,
        name,
        bundle,
        slug: slug_static,
        tagline,
        summary,
        website: None,
        features: &[],
        bright: (r, g, b),
    }
}

/// The repository's owner, uppercased for the little badge on the card.
pub fn owner_of(slug: &str) -> &str {
    slug.split('/').next().unwrap_or("GITUP")
}

/// The tile's background for an app whose own icon is not on this Mac —
/// a colour drawn from its id, so each app gets its own without anyone
/// choosing it by hand. The same id always answers the same colour, so
/// tiles do not jump around between launches.
pub fn tile_color(id: &str) -> (u8, u8, u8) {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in id.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    // Spread the bits so short ids still cover the wheel.
    hash ^= hash >> 15;
    let hue = (hash % 360) as f32;
    hsv_to_rgb(hue, 0.62, 0.74)
}

/// HSV in, sRGB out; the value kept down a little so a white letter
/// stays readable on top.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_u8 = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r), to_u8(g), to_u8(b))
}
