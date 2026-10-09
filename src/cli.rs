use std::path::PathBuf;

use clap::Parser;

/// Terminal UI for Google Tasks, backed by the gws CLI.
#[derive(Parser, Debug)]
#[command(name = "gws-tasks-tui", version, about)]
pub struct Args {
    /// Seconds between background refreshes (0 disables periodic refresh).
    #[arg(long, default_value_t = 60)]
    pub refresh_secs: u64,

    /// Path to the gws binary.
    #[arg(long, default_value = "gws")]
    pub gws_bin: PathBuf,

    /// Per-call timeout for gws, in seconds.
    #[arg(long, default_value_t = 30)]
    pub timeout_secs: u64,

    /// Use an in-memory mock backend instead of gws (no network).
    #[arg(long)]
    pub mock: bool,

    /// Simulated latency per mock call, in milliseconds.
    #[arg(long, default_value_t = 0, requires = "mock")]
    pub mock_latency_ms: u64,

    /// Make every Nth mock mutation fail, to exercise error handling.
    #[arg(long, requires = "mock")]
    pub mock_fail_every: Option<u32>,
}
