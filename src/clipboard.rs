//! System clipboard writes: the platform clipboard tool when one is installed,
//! otherwise an OSC 52 escape so the terminal sets the clipboard (also works
//! over SSH). Tools are run directly with the text on stdin, never via a shell.

/// Clipboard tools in preference order, with their arguments.
#[cfg(not(test))]
const TOOLS: &[(&str, &[&str])] = &[
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
];

#[cfg(not(test))]
pub fn copy(text: &str) -> Result<(), String> {
    copy_with(
        text,
        |text| {
            TOOLS
                .iter()
                .any(|(bin, args)| pipe_to(bin, args, text).is_ok())
        },
        |text| {
            crossterm::execute!(
                std::io::stdout(),
                crossterm::clipboard::CopyToClipboard::to_clipboard_from(text)
            )
        },
    )
}

/// Tests never touch the real clipboard; copies are recorded per thread instead.
#[cfg(test)]
pub fn copy(text: &str) -> Result<(), String> {
    COPIED.with(|copied| copied.borrow_mut().push(text.to_string()));
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub static COPIED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn copy_with(
    text: &str,
    tool: impl FnOnce(&str) -> bool,
    osc52: impl FnOnce(&str) -> std::io::Result<()>,
) -> Result<(), String> {
    if tool(text) {
        return Ok(());
    }
    osc52(text).map_err(|err| format!("could not copy to clipboard ({})", err.kind()))
}

#[cfg(not(test))]
fn pipe_to(bin: &str, args: &[&str], text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Dropping stdin closes the pipe so the tool sees EOF.
    let written = child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(text.as_bytes());
    let status = child.wait()?;
    written?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("{bin} exited with {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_clipboard_tool_and_falls_back_to_osc52() {
        let mut osc52_called = false;
        assert_eq!(
            copy_with(
                "hi",
                |_| true,
                |_| {
                    osc52_called = true;
                    Ok(())
                }
            ),
            Ok(())
        );
        assert!(!osc52_called);

        let mut sent = None;
        assert_eq!(
            copy_with(
                "hi",
                |_| false,
                |text| {
                    sent = Some(text.to_string());
                    Ok(())
                }
            ),
            Ok(())
        );
        assert_eq!(sent.as_deref(), Some("hi"));

        assert_eq!(
            copy_with(
                "hi",
                |_| false,
                |_| Err(std::io::ErrorKind::BrokenPipe.into())
            ),
            Err("could not copy to clipboard (broken pipe)".to_string())
        );
    }
}
