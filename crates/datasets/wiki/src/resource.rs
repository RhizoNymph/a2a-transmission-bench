//! The public URL of a wiki page, the channel resource agents read and
//! write through.
//!
//! The same URL string goes into both synthetic `http_request` calls (`GET`
//! to read, `POST` to write). Labels name the page by the bench's canonical
//! form of that URL ([`a2a_bench_resource::canonical_url`]), so a label and
//! a detector's prediction are compared on one canonicaliser.

use a2a_bench_format::resource::Resource;
use a2a_bench_resource::canonical_url;

/// The `https` URL of page `name` on `wiki`.
///
/// Hosts follow the export's own site list (`site-coverage.csv`): the `dse`,
/// `fractal` and `wiki4d` wikis live under `prowiki.org`, `dorfwiki` on
/// `dorfwiki.org`, the others under `wikiservice.at/<wiki>/`.
pub fn page_url(wiki: &str, name: &str) -> String {
    let (host, prefix) = match wiki {
        "dse" => ("www.prowiki.org", "/dse"),
        "fractal" => ("www.prowiki.org", "/fractal"),
        "wiki4d" => ("www.prowiki.org", "/wiki4d"),
        "dorfwiki" => ("www.dorfwiki.org", ""),
        other => return format!("https://www.wikiservice.at/{other}/{}", escape(name)),
    };
    format!("https://{host}{prefix}/{}", escape(name))
}

/// The canonical resource of page `name` on `wiki`, or `None` when the page
/// gets no channel labels.
///
/// crosstalk-eval refused a URL holding whitespace (its matcher's
/// `parse_url`), which is reachable only through a page name holding a tab
/// or a line break; those pages keep their exchanges but get no labels, as
/// there.
pub fn page_resource(wiki: &str, name: &str) -> Option<Resource> {
    let url = page_url(wiki, name);
    if url.chars().any(char::is_whitespace) {
        return None;
    }
    canonical_url(&url).ok()
}

/// Percent-encodes the characters of a page name that would otherwise break
/// the path or the URL parse (space, `#`, `?`, `%`).
fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '%' => out.push_str("%25"),
            other => out.push(other),
        }
    }
    out
}
