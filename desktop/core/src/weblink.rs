//! Web links sent between devices (`url` messages): only `http` and `https` are opened, so a
//! paired device can't make this PC run `file:`, `smb:` or an app's own scheme.

/// `text` when it is one `http(s)` link and nothing else, trimmed; else None.
pub fn parse(text: &str) -> Option<String> {
    let url = text.trim();
    if url.is_empty() || url.len() > 8192 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let scheme_end = url.find("://")?;
    let scheme = &url[..scheme_end];
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    (!host(url)?.is_empty()).then(|| url.to_string())
}

/// "example.com" in `https://user@example.com:8080/path`; None when it isn't a link.
pub fn host(url: &str) -> Option<&str> {
    let rest = &url[url.find("://")? + 3..];
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    // [::1]:80 keeps its brackets; example.com:80 loses the port.
    Some(match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().map(|h| &host[..h.len() + 2]).unwrap_or(host),
        None => host.split(':').next().unwrap_or(host),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_links_only() {
        assert_eq!(parse("  https://example.com/a?b=1&c=2 ").as_deref(), Some("https://example.com/a?b=1&c=2"));
        assert_eq!(parse("HTTP://example.com").as_deref(), Some("HTTP://example.com"));
        assert_eq!(parse("file:///etc/passwd"), None);
        assert_eq!(parse("javascript:alert(1)"), None);
        assert_eq!(parse("smb://server/share"), None);
        assert_eq!(parse("https://"), None);
        assert_eq!(parse("look at https://example.com"), None);
        assert_eq!(parse("example.com"), None);
    }

    #[test]
    fn hosts() {
        assert_eq!(host("https://user:pw@example.com:8080/x"), Some("example.com"));
        assert_eq!(host("https://example.com?q"), Some("example.com"));
        assert_eq!(host("http://[::1]:80/"), Some("[::1]"));
    }
}
