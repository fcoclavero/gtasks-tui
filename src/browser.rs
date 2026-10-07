//! Open explicit web links using the platform launcher, never an interpolated shell command.

use std::io;

use url::Url;

pub fn open_link(url: &str) -> Result<(), String> {
    open_with(url, |url| open::that(url))
}

fn open_with(url: &str, launch: impl FnOnce(&str) -> io::Result<()>) -> Result<(), String> {
    let invalid = || "only valid HTTP(S) links can be opened".to_string();
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(invalid());
    }
    let parsed = Url::parse(url).map_err(|_| invalid())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(invalid());
    }
    launch(parsed.as_str()).map_err(|err| format!("could not launch browser ({})", err.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_normalized_web_urls_without_shell_interpolation() {
        for url in [
            "https://docs.example/task?x=1&y=2#section",
            "http://example.test/a%20b",
            "https://example.test/$(do-not-execute);text",
        ] {
            let mut opened = None;
            open_with(url, |url| {
                opened = Some(url.to_string());
                Ok(())
            })
            .unwrap();
            assert_eq!(opened.as_deref(), Some(Url::parse(url).unwrap().as_str()));
        }
    }

    #[test]
    fn refuses_unsafe_or_invalid_urls_before_launching() {
        for url in [
            "",
            "javascript:alert(1)",
            "file:///tmp/attachment",
            "data:text/html,hello",
            "mailto:user@example.test",
            "--help",
            "https://",
            "https://example.test/a\nb",
            "https://example.test/a b",
        ] {
            assert!(
                open_with(url, |_| panic!("unsafe URL reached launcher")).is_err(),
                "accepted {url:?}"
            );
        }
    }

    #[test]
    fn reports_launch_failures_without_exposing_url_or_credentials() {
        let result = open_with("https://example.test/private", |_| {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "launcher missing: sensitive-url",
            ))
        });
        assert_eq!(
            result,
            Err("could not launch browser (entity not found)".into())
        );
    }
}
