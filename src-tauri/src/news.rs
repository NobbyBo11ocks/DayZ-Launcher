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

/// First `[img src="…"]` or `[img]…[/img]`, with `{STEAM_CLAN_IMAGE}` expanded;
/// only `https://` results are returned.
fn first_image(contents: &str) -> Option<String> {
    let src = if let Some(i) = contents.find("[img src=\"") {
        let start = i + "[img src=\"".len();
        let end = contents[start..].find('"')? + start;
        &contents[start..end]
    } else {
        let start = contents.find("[img]")? + "[img]".len();
        let end = contents[start..].find("[/img]")? + start;
        &contents[start..end]
    };
    let url = src.trim().replace("{STEAM_CLAN_IMAGE}", CLAN_IMAGE_BASE);
    (url.starts_with("https://") && !url.contains(char::is_whitespace)).then_some(url)
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
                } else if !last_space {
                    // Tags often separate lines or list items: keep the words apart.
                    out.push(' ');
                    last_space = true;
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
        Some(i) if i > max / 2 => &cut[..i],
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
    let body = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("news request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("news request failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("news reply unreadable: {e}"))?;
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
        assert_eq!(first_video("[previewyoutube=\"\"]"), None);
        assert_eq!(first_video("text"), None);
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
}
