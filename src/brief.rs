//! The daily brief — the pad's reading surface.
//!
//! One page, one day. Titles, sources, and the excerpt the feed already wrote.
//! No article bodies, no navigation, no fetching of arbitrary pages: the job
//! here is *selection*, and the feed upstream has already done it.
//!
//! A capture surface only runs half the loop — you bring a problem and nothing
//! comes back. This is the return path, on the same paper.
//!
//! Layout is decided by measuring, not by a fixed item count: items are laid
//! out until the next one would not fit, and whatever is left over is counted
//! on the page. Silent truncation reads as "that was everything" when it
//! wasn't.

use ab_glyph::FontRef;

use crate::page::{self, BODY_PX, LINE_H, TITLE_LINE_H};

/// One item on the brief.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub source: String,
    pub excerpt: String,
    pub date: String,
}

/// A fetched brief, as rendered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Brief {
    /// Human date for the header, e.g. "24 August 2026".
    pub date: String,
    /// Optional briefing line. The upstream briefing endpoint scrapes a third
    /// party and currently returns null, so this is a header when present and
    /// never the page itself.
    pub summary: Option<String>,
    pub items: Vec<Item>,
    /// True when this is the last brief we hold rather than a fresh fetch.
    pub stale: bool,
}

/// The gap under each item. Page geometry is shared (see `page`); this is the
/// brief's own rhythm, because a list of items breathes differently from a
/// single session.
const ITEM_GAP: usize = 30;

/// A laid-out item: the wrapped lines and the height they occupy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemLayout {
    pub title_lines: Vec<String>,
    pub meta: String,
    pub body_lines: Vec<String>,
    pub height: usize,
}

/// The whole page, measured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageLayout {
    pub laid_out: Vec<ItemLayout>,
    /// Items that did not fit. Shown as a count, never dropped silently.
    pub remaining: usize,
}

/// How many lines of excerpt an item gets. Enough to be worth reading, few
/// enough that one item cannot take the page.
const MAX_BODY_LINES: usize = 3;

pub fn layout_item(font: &FontRef, item: &Item) -> ItemLayout {
    let title_lines = page::title_lines(font, &item.title);
    let meta = page::meta_line(&item.source, &item.date);
    let body_lines = page::wrap_capped(font, &item.excerpt, BODY_PX, MAX_BODY_LINES);
    let height = title_lines.len() * TITLE_LINE_H
        + LINE_H // meta row
        + body_lines.len() * LINE_H
        + ITEM_GAP;
    ItemLayout { title_lines, meta, body_lines, height }
}

/// Lay out as many items as fit above the footer.
///
/// An item with no excerpt is skipped entirely: a headline with no body reads
/// as broken, and on paper you cannot scroll past it. (Hacker News items are
/// the live example — their RSS description is only the word "Comments".)
pub fn layout_page(font: &FontRef, brief: &Brief) -> PageLayout {
    let mut y = page::HEADER_H;
    if brief.summary.is_some() {
        y += LINE_H * 2;
    }
    // An item with no excerpt never reaches the page, so it is not "left out"
    // either — it is not showable at all.
    let showable: Vec<&Item> = brief.items.iter().filter(|i| !i.excerpt.trim().is_empty()).collect();
    let measured: Vec<ItemLayout> = showable.iter().map(|i| layout_item(font, i)).collect();
    let (laid_out, remaining) = page::fit(measured, y, page::limit(0), page::Fill::Front, |l| l.height);
    PageLayout { laid_out, remaining }
}

/// The footer line. Empty when everything fit — no need to say "0 more".
pub fn footer_label(layout: &PageLayout, stale: bool) -> String {
    let more = page::counted(layout.remaining, "more", "more");
    match (layout.remaining, stale) {
        (0, false) => String::new(),
        (0, true) => page::footer(&["last brief".into(), "not refreshed".into()]),
        (_, false) => more,
        (_, true) => page::footer(&[more, "last brief".into()]),
    }
}

/// Parse the intel feed payload.
///
/// Deliberately tolerant: a feed that adds fields must not break the pad, and
/// an item missing a title or excerpt is skipped rather than rendered blank.
pub fn parse_feed(json: &str) -> Vec<Item> {
    let mut items = Vec::new();
    for block in split_objects(json, "articles") {
        let title = json_field(&block, "title").unwrap_or_default();
        let excerpt = json_field(&block, "excerpt").unwrap_or_default();
        if title.trim().is_empty() || excerpt.trim().is_empty() {
            continue;
        }
        items.push(Item {
            id: json_field(&block, "id").unwrap_or_default(),
            title,
            source: json_field(&block, "source").unwrap_or_default(),
            excerpt,
            date: json_field(&block, "date").unwrap_or_default(),
        });
    }
    items
}

/// Split the named array into per-object slices. A hand-rolled scan keeps the
/// binary free of a JSON dependency for two endpoints.
///
/// Shared with the Claude bridge, which reads the same shape under different
/// keys — one scanner, so a fix to the escape handling lands on both readers.
pub(crate) fn split_objects(json: &str, array_key: &str) -> Vec<String> {
    let Some(start) = json.find(&format!("\"{array_key}\"")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    // Iterate chars, not bytes: the feed is UTF-8 and titles carry smart
    // quotes and em dashes. Casting a byte to char splits those into mojibake
    // ("won’t" becomes "wonâ€™t").
    for c in json[start..].chars() {
        if in_string {
            current.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                if depth > 0 {
                    current.push(c);
                }
            }
            '{' => {
                depth += 1;
                if depth == 1 {
                    current.clear();
                } else {
                    current.push(c);
                }
            }
            '}' => {
                if depth == 1 {
                    out.push(std::mem::take(&mut current));
                } else if depth > 1 {
                    current.push(c);
                }
                depth -= 1;
                if depth < 0 {
                    break;
                }
            }
            ']' if depth == 0 => break,
            _ => {
                if depth > 0 {
                    current.push(c);
                }
            }
        }
    }
    out
}

/// Read one string field out of a flat JSON object slice.
///
/// Shared with the Claude bridge for the same reason as `split_objects`.
pub(crate) fn json_field(block: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = block.find(&needle)? + needle.len();
    let rest = block[at..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push(' '),
                Some('t') => out.push(' '),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            _ => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UI_FONT_TTF;

    fn font() -> FontRef<'static> {
        FontRef::try_from_slice(UI_FONT_TTF).unwrap()
    }

    fn item(title: &str, excerpt: &str) -> Item {
        Item {
            id: "x".into(),
            title: title.into(),
            source: "irregulars".into(),
            excerpt: excerpt.into(),
            date: "today".into(),
        }
    }

    #[test]
    fn parses_the_live_feed_shape() {
        let json = r#"{"articles":[
          {"id":"a-1","source":"executivegov","title":"DIU Unveils Bridge Program",
           "excerpt":"The unit is standing up a program.","url":"https://x","date":"2026-08-24","tags":[]},
          {"id":"a-2","source":"irregulars","title":"Germany opens drone centre",
           "excerpt":"Saxony-Anhalt.","url":"https://y","date":"2026-08-24","tags":["news"]}
        ],"total":2}"#;
        let items = parse_feed(json);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "DIU Unveils Bridge Program");
        assert_eq!(items[0].source, "executivegov");
        assert_eq!(items[1].date, "2026-08-24");
    }

    #[test]
    fn skips_items_with_no_excerpt() {
        // Hacker News' RSS description is only "Comments"; upstream drops
        // those now, but the pad must not render a bodyless card either.
        let json = r#"{"articles":[
          {"id":"a","source":"hackernews","title":"Some post","excerpt":"","date":"2026-08-24"},
          {"id":"b","source":"irregulars","title":"Real story","excerpt":"Body text.","date":"2026-08-24"}
        ]}"#;
        let items = parse_feed(json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Real story");
    }

    #[test]
    fn decodes_escapes_and_unicode() {
        let json = r#"{"articles":[{"id":"a","source":"s","title":"Anthropic's model",
          "excerpt":"line one\nline two","date":"d"}]}"#;
        let items = parse_feed(json);
        assert_eq!(items[0].title, "Anthropic's model");
        assert_eq!(items[0].excerpt, "line one line two");
    }

    #[test]
    fn keeps_multibyte_characters_intact() {
        // The live feed carries smart quotes and em dashes. Scanning the JSON
        // byte-by-byte splits those into mojibake ("won’t" -> "wonâ€™t").
        let json = "{\"articles\":[{\"id\":\"a\",\"source\":\"s\",\
            \"title\":\"Army wants agents that won\u{2019}t run up token costs\",\
            \"excerpt\":\"Osan drills target North Korea\u{2019}s drone threat \u{2014} officials say.\",\
            \"date\":\"2026-08-24\"}]}";
        let items = parse_feed(json);
        assert_eq!(items.len(), 1);
        assert!(items[0].title.contains('\u{2019}'), "title mangled: {}", items[0].title);
        assert!(!items[0].title.contains('\u{fffd}'));
        assert!(items[0].excerpt.contains('\u{2014}'), "excerpt mangled: {}", items[0].excerpt);
    }

    #[test]
    fn tolerates_junk_and_missing_fields() {
        assert!(parse_feed("").is_empty());
        assert!(parse_feed("{}").is_empty());
        assert!(parse_feed(r#"{"articles":[]}"#).is_empty());
        assert!(parse_feed(r#"{"articles":[{"source":"s"}]}"#).is_empty());
    }

    #[test]
    fn a_page_stops_before_the_footer_and_counts_the_rest() {
        let f = font();
        let many: Vec<Item> = (0..40)
            .map(|i| {
                item(
                    &format!("Headline number {i} about defense acquisition and autonomy"),
                    "An excerpt long enough to wrap across more than one line of the page so \
                     that each item takes real vertical space on the display.",
                )
            })
            .collect();
        let brief = Brief { date: "24 August 2026".into(), summary: None, items: many, stale: false };
        let page = layout_page(&f, &brief);

        assert!(!page.laid_out.is_empty(), "nothing fit");
        assert!(page.remaining > 0, "40 items should not all fit");
        assert_eq!(page.laid_out.len() + page.remaining, 40);

        let used: usize = page.laid_out.iter().map(|l| l.height).sum();
        assert!(used + page::HEADER_H <= page::limit(0), "page overflows the footer");
    }

    #[test]
    fn everything_fitting_leaves_no_footer_count() {
        let f = font();
        let brief = Brief {
            date: "24 August 2026".into(),
            summary: None,
            items: vec![item("Short", "A brief excerpt.")],
            stale: false,
        };
        let page = layout_page(&f, &brief);
        assert_eq!(page.remaining, 0);
        assert_eq!(footer_label(&page, false), "");
    }

    #[test]
    fn a_stale_brief_says_so() {
        let page = PageLayout { laid_out: Vec::new(), remaining: 0 };
        assert_eq!(footer_label(&page, true), "last brief · not refreshed");
        let page = PageLayout { laid_out: Vec::new(), remaining: 4 };
        assert_eq!(footer_label(&page, true), "4 more · last brief");
        assert_eq!(footer_label(&page, false), "4 more");
    }

    #[test]
    fn a_summary_reduces_the_room_for_items() {
        let f = font();
        let items: Vec<Item> = (0..40)
            .map(|i| item(&format!("Headline {i} about acquisition"), "Excerpt text that wraps a little."))
            .collect();
        let without = layout_page(&f, &Brief {
            date: "d".into(), summary: None, items: items.clone(), stale: false });
        let with = layout_page(&f, &Brief {
            date: "d".into(), summary: Some("Today in Washington.".into()), items, stale: false });
        assert!(with.laid_out.len() <= without.laid_out.len());
    }

    #[test]
    fn long_titles_and_excerpts_are_capped() {
        let f = font();
        let long = "word ".repeat(200);
        let layout = layout_item(&f, &item(&long, &long));
        assert!(layout.title_lines.len() <= page::MAX_TITLE_LINES);
        assert!(layout.body_lines.len() <= MAX_BODY_LINES);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::ui::UI_FONT_TTF;

    /// Lay out the real feed, if a snapshot is present. Skipped otherwise so
    /// the suite stays hermetic.
    #[test]
    fn real_feed_lays_out_a_sensible_page() {
        let Ok(json) = std::env::var("GPAD_FEED_FIXTURE").and_then(|p| {
            std::fs::read_to_string(p).map_err(|_| std::env::VarError::NotPresent)
        }) else { return };
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let items = parse_feed(&json);
        assert!(items.len() > 10, "expected a real feed, got {}", items.len());
        let brief = Brief { date: "24 August 2026".into(), summary: None, items, stale: false };
        let page = layout_page(&font, &brief);
        eprintln!("LIVE: {} items parsed, {} laid out, {} remaining",
            brief.items.len(), page.laid_out.len(), page.remaining);
        for l in &page.laid_out {
            eprintln!("  [{}] {}", l.meta, l.title_lines.join(" "));
        }
        assert!(page.laid_out.len() >= 3, "too few items fit: {}", page.laid_out.len());
        assert!(page.laid_out.len() <= 12, "suspiciously many fit");
    }
}
