# gws-tasks-tui

A small terminal UI for Google Tasks, written in Rust with [Ratatui](https://ratatui.rs). All Google traffic goes through the [Google Workspace CLI (`gws`)](https://github.com/googleworkspace/cli), so authentication, scopes, and account switching stay in one place.

## Prerequisites

- Rust 1.98.1 or newer (edition 2024). Tested with Rust 1.98.1.
- `gws` on your `PATH` (or pass `--gws-bin`), logged in with the Tasks scope:

```sh
gws auth login --services tasks
```

Re-logging in replaces the stored token, so include every service you use (for example `--services tasks,gmail,calendar`). Unverified OAuth apps in testing mode are limited to roughly 25 scopes.

Account switching works the same way it does for `gws`: set `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` before launching and the TUI inherits it. The status bar shows the config directory name (or `default`).

## Install

Once published to crates.io:

```sh
cargo install gws-tasks-tui --locked
```

From a local checkout:

```sh
cargo install --path . --locked
```

## Usage

```sh
gws-tasks-tui                      # live data through gws, refresh every 60 s
gws-tasks-tui --refresh-secs 30     # faster periodic refresh (0 disables it)
gws-tasks-tui --mock                # seeded in-memory data, no network, no gws
gws-tasks-tui --mock --mock-fail-every 2 --mock-latency-ms 400
```

## Layout

One column per task list. Subtasks are nested under their parent. Each row shows the title and the due date (`MM-DD` this year, full date otherwise).
The first nine columns are numbered left to right, with `[1]`–`[9]` prefixes in their titles. Press the corresponding number to jump to a list, even when it is off-screen. Additional columns have no number and remain accessible with the left/right keys.

- Green: completed.
- Red: overdue (due date before today, not completed).
- White: everything else.
- Dimmed: a completed parent that is only shown because one of its subtasks is visible.

Completed tasks are hidden by default. The column title shows how many are hidden.

Press `s` to toggle all lists between Google's position order (the default) and due-date order, earliest first, with undated tasks last. Subtasks stay under their parent and are sorted by their own dates. Tasks with the same date keep their position order. The status bar shows the active sort mode; sorting only changes the display, not Google's stored order.

## Keys

| Key | Action |
| ------------- | ------------------------------------------------------------- |
| `←` `→` `h` `l` | Switch task list |
| `1`–`9` | Jump to the corresponding task list, numbered left to right |
| `↑` `↓` `j` `k` | Move between tasks and subtasks |
| `g` `G` | First / last task |
| `Space` | Toggle done. The task stays visible until you move away, so you can undo. |
| `Enter` | Edit title, due date, and notes. Links are shown read-only. |
| `c` | Show / hide completed tasks |
| `s` | Toggle position / due-date sorting |
| `n` `N` | New task / new subtask under the selected task |
| `D` | Delete the selected task and its subtasks, after confirmation |
| `r` | Refresh now |
| `?` | Help |
| `q` | Quit (waits up to 2 s for in-flight changes) |

In the new-task prompt: `Tab` switches to creating a subtask under the selected task, keeping the title you have typed. The prompt names the parent; `Enter` creates the task and `Esc` cancels. If a subtask is selected, the new subtask goes under the same parent (Google Tasks supports one nesting level).

In the editor: `Tab` / `Shift-Tab` move between fields, `Enter` moves to the next field (inserts a newline in Notes), `Ctrl-S` saves, `Ctrl-U` clears the due date, `Esc` cancels (twice if there are unsaved changes).
Click **Open task in browser** at the bottom of the editor, or press `Ctrl-O`, to open the task's web link in your default browser without closing the editor or saving changes. If Google does not provide a `webViewLink`, the editor shows **Task link unavailable**.

### Due-date input

- Type an exact date with or without dashes (`2026-12-24` or `20261224`). Digits overwrite the fixed `YYYY-MM-DD` mask; dashes are always present and cannot be edited. `←` / `→` skip them, and `Home` / `End` jump to the first / last digit. `Backspace` / `Delete` clear digits without removing the separators.
- Type `today`, `tomorrow`, a weekday (`fri` or `friday`), `next mon`, or an offset (`+3d`, `+2w`, or `-1d`). Typing a letter or `+` replaces the numeric date with shorthand; use `Ctrl-U` to start fresh. A live preview shows the resolved date before saving.
- Weekdays mean the next occurrence, including today; `next <weekday>` always means a future occurrence. Relative dates use the local day when the editor opened, keeping the preview and saved date consistent.
- `↑` / `↓` increase / decrease the year, month, or day under the cursor by one. When the field is empty, the first press sets today and selects the day; subsequent presses adjust it. With shorthand, arrows resolve it into a numeric date and adjust the day.
- Month / year adjustments clamp to the last valid day (January 31 becomes February 28 or 29); day adjustments cross month / year boundaries. `Shift-↑` / `Shift-↓` adjust by a week when the cursor is in the day segment.
- `Ctrl-U` clears the date, leaving an empty `____-__-__` mask. Incomplete or invalid dates block saving.

## Behaviour notes

- Changes apply immediately in the UI and are sent to Google in the background. If a call fails, the change is reverted and the error is shown in the status bar.
- A background refresh never overwrites a list that has changes in flight, never resets the selection, and never touches an open editor.
- The public Tasks API does not expose stars or Drive attachments, so neither is shown. The read-only `links` field (Gmail, Docs, or Chat origins) is listed in the editor.
- If the token lacks the Tasks scope, the UI stays open and the status bar tells you to run `gws auth login --services tasks`.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -- --mock
```

`tests/fixtures/` holds recorded `gws` output shapes (with fictitious ids) used by the parser tests. Run `cargo audit` (install with `cargo install cargo-audit`) before publishing a release.

## Release validation

Following the [Cargo publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html#packaging-a-crate), verify that the packaged sources build before publishing:

```sh
cargo publish --dry-run --locked
cargo package --list --locked
```

The dry run packages the crate and builds the extracted sources without uploading anything to crates.io. For uncommitted local changes, add `--allow-dirty` to the dry-run command.

## License

Licensed under the [MIT License](LICENSE).
