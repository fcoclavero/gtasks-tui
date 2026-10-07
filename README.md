# gtasks-tui

A small terminal UI for Google Tasks, written in Rust with [Ratatui](https://ratatui.rs). All Google traffic goes through the [Google Workspace CLI (`gws`)](https://github.com/googleworkspace/cli), so authentication, scopes, and account switching stay in one place.

## Prerequisites

- Rust 1.98 or newer (edition 2024).
- `gws` on your `PATH` (or pass `--gws-bin`), logged in with the Tasks scope:

```sh
gws auth login --services tasks
```

Re-logging in replaces the stored token, so include every service you use (for example `--services tasks,gmail,calendar`). Unverified OAuth apps in testing mode are limited to roughly 25 scopes.

Account switching works the same way it does for `gws`: set `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` before launching and the TUI inherits it. The status bar shows the config directory name (or `default`).

## Install

```sh
cargo install --path .
```

## Usage

```sh
gtasks-tui                      # live data through gws, refresh every 60 s
gtasks-tui --refresh-secs 30    # faster periodic refresh (0 disables it)
gtasks-tui --mock               # seeded in-memory data, no network, no gws
gtasks-tui --mock --mock-fail-every 2 --mock-latency-ms 400
```

## Layout

One column per task list. Subtasks are nested under their parent. Each row shows the title and the due date (`MM-DD` this year, full date otherwise).

- Green: completed.
- Red: overdue (due date before today, not completed).
- White: everything else.
- Dimmed: a completed parent that is only shown because one of its subtasks is visible.

Completed tasks are hidden by default. The column title shows how many are hidden.

## Keys

| Key | Action |
| ------------- | ------------------------------------------------------------- |
| `←` `→` `h` `l` | Switch task list |
| `↑` `↓` `j` `k` | Move between tasks and subtasks |
| `g` `G` | First / last task |
| `Space` | Toggle done. The task stays visible until you move away, so you can undo. |
| `Enter` | Edit title, due date, and notes. Links are shown read-only. |
| `c` | Show / hide completed tasks |
| `n` `N` | New task / new subtask under the selected task |
| `D` | Delete the selected task and its subtasks, after confirmation |
| `r` | Refresh now |
| `?` | Help |
| `q` | Quit (waits up to 2 s for in-flight changes) |

In the editor: `Tab` / `Shift-Tab` move between fields, `Enter` moves to the next field (inserts a newline in Notes), `Ctrl-S` saves, `Ctrl-U` clears the due date, `Esc` cancels (twice if there are unsaved changes).

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
