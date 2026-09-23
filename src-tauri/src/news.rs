//! DayZ news from Steam's keyless news feed (S-67, D-099): Bohemia's community
//! announcements (updates, dev blogs, sales) plus the third-party press feeds Steam
//! attaches to the app. Summaries are reduced to plain text here so the WebView
//! never renders remote markup.

use serde::{Deserialize, Serialize};

const NEWS_URL: &str = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/";
/// Feed name of Bohemia's own posts; everything else is press coverage.
pub const OFFICIAL_FEED: &str = "steam_community_announcements";
const USER_AGENT: &str = concat!(
    "DayZLauncher/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/NobbyBo11ocks/dayz-launcher)"
);
/// Characters of body text kept per post.
const SUMMARY_CHARS: usize = 320;
/// Steam expands `{STEAM_CLAN_IMAGE}` in announcement bodies to this base (S-69).
const CLAN_IMAGE_BASE: &str = "https://clan.akamai.steamstatic.com/images";
/// Hosts a post picture may be fetched from for a thumbnail: Steam's clan-image CDN only.
const IMAGE_HOSTS: [&str; 2] = ["clan.akamai.steamstatic.com", "clan.fastly.steamstatic.com"];
/// Longest side of a cached thumbnail; a card is at most about 640 px wide.
const THUMB_MAX: u32 = 640;
/// A source picture above this size is refused (the CDN serves 4–5 MB JPEGs, S-72).
const IMAGE_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
struct Document {
    appnews: AppNews,
}

#[derive(Deserialize)]
struct AppNews {
    #[serde(default)]
    newsitems: Vec<RawItem>,
}

#[derive(Deserialize)]
struct RawItem {
    gid: String,
    title: String,
    url: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    contents: String,
    #[serde(default)]
    feedlabel: String,
    #[serde(default)]
    feedname: String,
    date: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub gid: String,
    pub title: String,
    /// Steam's store news page for official posts, the original article otherwise.
    pub url: String,
    pub author: String,
    /// Feed label as Steam shows it ("Community Announcements", "PC Gamer", …).
    pub feed: String,
    /// From Bohemia's own announcement feed.
    pub official: bool,
    /// Unix seconds.
    pub date: i64,
    pub summary: String,
    /// The title reads like a game update (patch, hotfix, experimental/stable release).
    pub update: bool,
    /// First picture in the post, on Steam's clan-image CDN (D-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// YouTube id of the first embedded video preview (D-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
}

/// First usable picture: `[img src="…"]` or `[img]…[/img]` with `{STEAM_CLAN_IMAGE}`
/// expanded, `https://` only. GIFs are skipped: Steam posts open with a 43-byte
/// transparent spacer GIF (S-69), which would render as a blank card.
fn first_image(contents: &str) -> Option<String> {
    let mut rest = contents;
    while let Some(i) = rest.find("[img") {
        let tag = &rest[i..];
        let (src, consumed) = if let Some(t) = tag.strip_prefix("[img src=\"") {
            let end = t.find('"')?;
            (&t[..end], i + "[img src=\"".len() + end)
        } else if let Some(t) = tag.strip_prefix("[img]") {
            let end = t.find("[/img]")?;
            (&t[..end], i + "[img]".len() + end)
        } else {
            rest = &rest[i + 4..];
            continue;
        };
        let url = src.trim().replace("{STEAM_CLAN_IMAGE}", CLAN_IMAGE_BASE);
        let usable = url.starts_with("https://")
            && !url.contains(char::is_whitespace)
            && !url.to_ascii_lowercase().ends_with(".gif");
        if usable {
            return Some(url);
        }
        rest = &rest[consumed..];
    }
    None
}

/// YouTube id from the first `[previewyoutube="ID;full"]` (S-69).
fn first_video(contents: &str) -> Option<String> {
    let start = contents.find("[previewyoutube=")? + "[previewyoutube=".len();
    let rest = contents[start..].trim_start_matches('"');
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!id.is_empty() && id.len() <= 16).then_some(id)
}

/// Game-update posts by title: what the "Updates only" filter and the update alert use.
pub fn is_update_title(title: &str) -> bool {
    let t = title.to_lowercase();
    [
        "update",
        "hotfix",
        "patch",
        "experimental",
        "stable",
        "release",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// Strips BBCode (`[b]`, `[url=…]`, `[list]`…) and HTML tags, decodes the common
/// entities, collapses whitespace and cuts at `max` characters on a word boundary.
pub fn plain_text(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len().min(max + 16));
    let mut chars = s.chars().peekable();
    let mut last_space = true;
    while let Some(c) = chars.next() {
        match c {
            '[' | '<' => {
                let close = if c == '[' { ']' } else { '>' };
                // Skip to the closing bracket; an unclosed one is kept as text.
                let mut skipped = String::new();
                let mut closed = false;
                for n in chars.by_ref() {
                    if n == close {
                        closed = true;
                        break;
                    }
                    skipped.push(n);
                }
                if !closed {
                    out.push(c);
                    out.push_str(&skipped);
                } else {
                    // `[img]url[/img]` carries a bare picture path as its content (the
                    // 1.27/1.28 posts, D-133): drop it with the tag. Without a closing
                    // tag the text is kept as it was.
                    if c == '[' && skipped == "img" {
                        let mut inner = String::new();
                        for n in chars.by_ref() {
                            inner.push(n);
                            if inner.ends_with("[/img]") {
                                inner.clear();
                                break;
                            }
                        }
                        if !inner.is_empty() {
                            out.push_str(&inner);
                            last_space = inner.ends_with(char::is_whitespace);
                        }
                    }
                    if !last_space {
                        // Tags often separate lines or list items: keep the words apart.
                        out.push(' ');
                        last_space = true;
                    }
                }
            }
            '&' => {
                // An entity is `&name;` or `&#nnn;`; anything else stays literal.
                let mut ent = String::new();
                while let Some(&n) = chars.peek() {
                    if (n.is_ascii_alphanumeric() || n == '#') && ent.len() < 8 {
                        ent.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let rep = if chars.peek() == Some(&';') {
                    match ent.as_str() {
                        "amp" => Some("&"),
                        "quot" => Some("\""),
                        "apos" | "#39" => Some("'"),
                        "lt" => Some("<"),
                        "gt" => Some(">"),
                        "nbsp" | "#160" => Some(" "),
                        _ => None,
                    }
                } else {
                    None
                };
                match rep {
                    Some(" ") => {
                        chars.next();
                        if !last_space {
                            out.push(' ');
                            last_space = true;
                        }
                    }
                    Some(r) => {
                        chars.next();
                        out.push_str(r);
                        last_space = false;
                    }
                    None => {
                        out.push('&');
                        out.push_str(&ent);
                        last_space = false;
                    }
                }
            }
            c if c.is_whitespace() => {
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            }
            c => {
                out.push(c);
                last_space = false;
            }
        }
    }
    let text = out.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        // `i` is a byte offset and `max` counts characters: a 20-character Cyrillic
        // cut is 38 bytes, so the word-boundary rule fired on the wrong condition for
        // any non-ASCII post (D-197).
        Some(i) if cut[..i].chars().count() > max / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', '.', ':', ';']))
}

fn convert(raw: RawItem) -> NewsItem {
    let official = raw.feedname == OFFICIAL_FEED;
    let url = if official {
        format!(
            "https://store.steampowered.com/news/app/221100/view/{}",
            raw.gid
        )
    } else {
        raw.url
    };
    NewsItem {
        update: official && is_update_title(&raw.title),
        image: first_image(&raw.contents),
        video: first_video(&raw.contents),
        summary: plain_text(&raw.contents, SUMMARY_CHARS),
        feed: if raw.feedlabel.is_empty() {
            raw.feedname
        } else {
            raw.feedlabel
        },
        gid: raw.gid,
        title: plain_text(&raw.title, 200),
        url,
        author: raw.author,
        official,
        date: raw.date,
    }
}

/// Only `https://` URLs on Steam's clan-image hosts qualify for a thumbnail.
pub fn image_allowed(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        IMAGE_HOSTS.iter().any(|h| {
            rest.strip_prefix(h)
                .is_some_and(|tail| tail.starts_with('/'))
        })
    })
}

/// Decodes a picture and re-encodes it as a JPEG of at most [`THUMB_MAX`] px on
/// the long side (aspect kept). CPU-bound: call from a blocking task.
pub fn shrink(data: &[u8], max: u32) -> Result<Vec<u8>, String> {
    // Without limits a small file declaring huge dimensions allocates the whole
    // raster before `thumbnail` ever downscales it (D-160).
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let reader = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("picture unreadable: {e}"))?;
    let mut reader = reader;
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| format!("picture unreadable: {e}"))?;
    let small = img.thumbnail(max, max).to_rgb8();
    let mut out = Vec::with_capacity(64 * 1024);
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
    small
        .write_with_encoder(enc)
        .map_err(|e| format!("thumbnail encoding failed: {e}"))?;
    Ok(out)
}

/// A JPEG thumbnail of a post's picture, cached as `dir/<key>.jpg` (D-111). The
/// source is typically 3840×2160 and 4.6 MB (S-72); the WebView would decode that
/// to 33 MB per card, so it is shrunk once here and the small file is served after.
pub async fn thumbnail(
    url: String,
    dir: std::path::PathBuf,
    key: String,
    max: u32,
) -> Result<Vec<u8>, String> {
    if !image_allowed(&url) {
        return Err("picture host not allowed".into());
    }
    let safe: String = key.chars().filter(char::is_ascii_alphanumeric).collect();
    if safe.is_empty() {
        return Err("bad thumbnail key".into());
    }
    // A card is ~340 px wide and the featured picture ~2x that, so the two sizes
    // are cached separately: one 640 px file per card decoded to 920 KB in the
    // WebView, ~22 MB for a full page (D-160).
    let max = max.clamp(160, THUMB_MAX);
    let path = dir.join(format!("{safe}-{max}.jpg"));
    if let Ok(bytes) = std::fs::read(&path) {
        return Ok(bytes);
    }
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        // `image_allowed` vetted the host, so following a redirect off it would
        // undo that check and turn this into a blind request to anywhere (D-160).
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("picture request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("picture request failed: {e}"))?;
    // The size was checked after the whole body was already in memory, so it
    // bounded nothing; the cap now applies while it downloads (D-160).
    let body = crate::http::body_capped(resp, IMAGE_MAX_BYTES, "picture").await?;
    tokio::task::spawn_blocking(move || {
        let bytes = shrink(&body, max)?;
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, &bytes);
        Ok(bytes)
    })
    .await
    .map_err(|e| format!("thumbnail task failed: {e}"))?
}

/// Drops cached thumbnails whose post is no longer in the list.
pub fn prune_thumbnails(dir: &std::path::Path, keep: &std::collections::HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let full = name.to_string_lossy();
        // Only ever a thumbnail: the unsuffixed branch below deletes on sight, and a
        // half-written `.tmp` in this directory is not ours to remove (D-197).
        let Some(stem) = full.strip_suffix(".jpg") else {
            continue;
        };
        // "<gid>-<max>.jpg" since D-160. A file with no suffix predates that and can
        // never be found again whatever its gid, but `rsplit_once` returning `None`
        // left the whole stem in place — a live gid — so the sweep kept precisely the
        // files it was written to remove: 18 of 37 here, 1 031 165 of 1 540 316 bytes
        // the app could no longer read (D-193).
        let Some((gid, _)) = stem.rsplit_once('-') else {
            let _ = std::fs::remove_file(entry.path());
            continue;
        };
        if !keep.contains(gid) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Latest `count` posts, newest first, with full bodies (`maxlength=0`) so the
/// pictures and video previews further down a post are found; the gzip reply for
/// 60 posts is well under 100 KB.
pub async fn fetch(count: u32) -> Result<Vec<NewsItem>, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    // Plain integers and fixed words only, so the URL is built without the `query`
    // and `json` reqwest features (kept off to stay with the updater's feature set).
    let url = format!(
        "{NEWS_URL}?appid={}&count={count}&maxlength=0&format=json",
        crate::steam::DAYZ_APP_ID
    );
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("news request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("news request failed: {e}"))?;
    // 60 posts are well under 100 KB gzipped; 8 MB is far above any honest reply (D-160).
    let body = crate::http::body_capped(resp, 8 * 1024 * 1024, "news reply").await?;
    let doc: Document =
        serde_json::from_slice(&body).map_err(|e| format!("news reply unreadable: {e}"))?;
    let mut items: Vec<NewsItem> = doc.appnews.newsitems.into_iter().map(convert).collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.date));
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markup_and_cuts() {
        let body = "[h1]Dear Survivors,[/h1]We are [b]thrilled[/b] to announce that&nbsp;Update 1.30 is now entering the [url=https://example.com]Experimental[/url] phase.<br><img src=\"x\">Wishlist &amp; enjoy.";
        assert_eq!(
            plain_text(body, 500),
            "Dear Survivors, We are thrilled to announce that Update 1.30 is now entering the Experimental phase. Wishlist & enjoy."
        );
        let cut = plain_text("one two three four five six", 12);
        assert_eq!(cut, "one two…");
        assert_eq!(plain_text("a [unclosed tag", 100), "a [unclosed tag");
        assert_eq!(plain_text("R&D &amp; more", 100), "R&D & more");
        // The 1.27/1.28 posts open with a bare-URL image tag (D-133).
        assert_eq!(
            plain_text(
                "[img]{STEAM_CLAN_IMAGE}/4458811/c72fc93abb14e43ab6fdd3c5b3924136206efb3d.jpg[/img]\nGreetings, Survivors!\nWe’re about to enter",
                500
            ),
            "Greetings, Survivors! We’re about to enter"
        );
        assert_eq!(plain_text("[img src=\"x\"][/img]Hello", 100), "Hello");
        assert_eq!(plain_text("[img]no closing tag", 100), "no closing tag");
    }

    #[test]
    fn update_titles() {
        assert!(is_update_title(
            "1.30 Experimental Release | Motorbikes, Inventory UI Improvements, and more!"
        ));
        assert!(is_update_title("DayZ | 1.28 Stable Update - Out Now!"));
        assert!(is_update_title("Hotfix 1.28.159874"));
        assert!(!is_update_title("PvE Survival Crafting Fest Sale"));
        assert!(!is_update_title(
            "DayZ Badlands | Dev Blog (Week 84) | Motorbikes&Ragdoll Physics | Wishlist now!"
        ));
    }

    #[test]
    fn finds_pictures_and_videos() {
        let body = "[p][img src=\"{STEAM_CLAN_IMAGE}/4458811/983b150f2889d08009135f645dce5fb986b09d81.jpg\"][/img][/p][previewyoutube=\"0_pk9N7FKIE;full\"][/previewyoutube]";
        assert_eq!(
            first_image(body).as_deref(),
            Some("https://clan.akamai.steamstatic.com/images/4458811/983b150f2889d08009135f645dce5fb986b09d81.jpg")
        );
        assert_eq!(first_video(body).as_deref(), Some("0_pk9N7FKIE"));
        assert_eq!(
            first_image("[img]https://example.com/a.png[/img]").as_deref(),
            Some("https://example.com/a.png")
        );
        assert_eq!(
            first_image("[img src=\"http://insecure/a.png\"][/img]"),
            None
        );
        assert_eq!(first_image("no pictures here"), None);
        // Steam's transparent spacer GIF comes first in many posts: take the next picture.
        assert_eq!(
            first_image("[img src=\"{STEAM_CLAN_IMAGE}/4458811/30cb5d44e276505b1d4c053c8b25525da228db30.gif\"][/img][p]text[/p][img src=\"{STEAM_CLAN_IMAGE}/4458811/real.png\"][/img]").as_deref(),
            Some("https://clan.akamai.steamstatic.com/images/4458811/real.png")
        );
        assert_eq!(
            first_image("[img src=\"{STEAM_CLAN_IMAGE}/4458811/only.gif\"][/img]"),
            None
        );
        assert_eq!(first_video("[previewyoutube=\"\"]"), None);
        assert_eq!(first_video("text"), None);
    }

    #[test]
    fn thumbnails_shrink_and_hosts_are_checked() {
        assert!(image_allowed(
            "https://clan.akamai.steamstatic.com/images/4458811/a.jpg"
        ));
        assert!(image_allowed(
            "https://clan.fastly.steamstatic.com/images/x.png"
        ));
        assert!(!image_allowed(
            "https://clan.akamai.steamstatic.com.evil.com/x.jpg"
        ));
        assert!(!image_allowed(
            "http://clan.akamai.steamstatic.com/images/x.jpg"
        ));
        assert!(!image_allowed("https://i.ytimg.com/vi/x/mqdefault.jpg"));

        // A 1920×1080 PNG becomes a JPEG no wider than 640 px with the aspect kept.
        let mut png = Vec::new();
        image::RgbImage::from_fn(1920, 1080, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        })
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
        let jpeg = shrink(&png, THUMB_MAX).unwrap();
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (640, 360));
        // Far below the 640×360×3 raw size, and nothing like the 33 MB a 4K decode costs.
        assert!(jpeg.len() < 640 * 360 * 3 / 4, "{} bytes", jpeg.len());
        assert!(shrink(b"not a picture", THUMB_MAX).is_err());
    }

    #[test]
    fn converts_official_and_press() {
        let official = convert(RawItem {
            gid: "1844115010488662".into(),
            title: "1.30 Experimental Release".into(),
            url: "https://steamstore-a.akamaihd.net/news/externalpost/steam_community_announcements/1844115010488662".into(),
            author: "lynn.zaw".into(),
            contents: "Dear Survivors,We are thrilled".into(),
            feedlabel: "Community Announcements".into(),
            feedname: OFFICIAL_FEED.into(),
            date: 1_789_553_289,
        });
        assert!(official.official && official.update);
        assert_eq!(
            official.url,
            "https://store.steampowered.com/news/app/221100/view/1844115010488662"
        );
        assert_eq!(official.feed, "Community Announcements");
        let press = convert(RawItem {
            gid: "1".into(),
            title: "DayZ gets bikes".into(),
            url: "https://www.pcgamer.com/x".into(),
            author: String::new(),
            contents: String::new(),
            feedlabel: "PC Gamer".into(),
            feedname: "PC Gamer".into(),
            date: 1,
        });
        assert!(!press.official && !press.update);
        assert_eq!(press.url, "https://www.pcgamer.com/x");
    }

    #[test]
    fn prune_drops_the_files_that_predate_the_suffix() {
        // The regression D-193 fixed: an unsuffixed file carries a live gid, so the
        // old rule looked it up in , found it, and kept a file nothing can read.
        let dir = std::env::temp_dir().join(format!("dzl-prune-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        for name in ["111-640.jpg", "111.jpg", "222-640.jpg", "333-640.jpg"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let keep: std::collections::HashSet<String> =
            ["111".to_string(), "222".to_string()].into_iter().collect();
        super::prune_thumbnails(&dir, &keep);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            left,
            vec!["111-640.jpg".to_string(), "222-640.jpg".to_string()]
        );
    }
}
