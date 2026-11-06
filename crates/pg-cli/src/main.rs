use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use pg_capture::{CaptureBackend, MacCapture, SystemTarget, TrackSummary};
use pg_dsp::align::{Alignment, parse_timing};
use pg_dsp::clicks::{click_offsets, find_clicks};

/// A mic click and a system click this close together are the same sound.
const CLICK_MATCH_MS: f64 = 500.0;

/// postgame: bot-free, local-first conversation intelligence.
#[derive(Parser)]
#[command(name = "pg")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Record the mic and system audio into two tracks. Ctrl-C stops.
    Record(RecordArgs),
    /// Measure the mic-to-system offset at each click in a recording.
    SyncCheck { dir: PathBuf },
}

#[derive(Args)]
struct RecordArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Directory for mic.wav, system.wav, timing.jsonl and health.jsonl.
    #[arg(long)]
    out: PathBuf,
    /// Stop automatically after this many seconds.
    #[arg(long)]
    seconds: Option<u64>,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct TargetArgs {
    /// Bundle ID prefix of the meeting app, e.g. us.zoom.xos or com.google.Chrome.
    #[arg(long)]
    app: Option<String>,
    /// Tap one process ID.
    #[arg(long)]
    pid: Option<i32>,
    /// Tap all system audio except this process.
    #[arg(long)]
    all_system: bool,
}

impl TargetArgs {
    fn target(self) -> SystemTarget {
        match (self.app, self.pid) {
            (Some(prefix), _) => SystemTarget::BundlePrefix(prefix),
            (_, Some(pid)) => SystemTarget::Pid(pid),
            _ => SystemTarget::AllSystem,
        }
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn version() -> String {
    format!(
        "{} (PGAudio {})",
        env!("CARGO_PKG_VERSION"),
        pg_capture::shim_version()
    )
}

fn print_track(name: &str, track: &TrackSummary) {
    println!(
        "{name}: {} samples, {} padded, peak {:.3}",
        track.samples, track.padded_samples, track.peak
    );
}

fn record(args: RecordArgs) -> Result<()> {
    let mut capture = MacCapture::new(args.out.clone());
    capture.start(args.target.target())?;
    println!("Recording to {}. Press Ctrl-C to stop.", args.out.display());

    let (stop, stopped) = channel();
    ctrlc::set_handler(move || {
        let _ = stop.send(());
    })?;
    match args.seconds {
        Some(seconds) => match stopped.recv_timeout(Duration::from_secs(seconds)) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err("Ctrl-C handler went away".into()),
        },
        None => stopped.recv()?,
    }

    let summary = capture.stop()?;
    print_track("mic", &summary.mic);
    print_track("system", &summary.system);
    if summary.system.peak == 0.0 {
        eprintln!(
            "warning: the system track is silent. If the app was playing audio, allow your \
             terminal in System Settings > Privacy & Security > Screen & System Audio Recording."
        );
    }
    Ok(())
}

fn read_wav(path: PathBuf) -> Result<(Vec<f32>, f64)> {
    let mut reader = hound::WavReader::open(path)?;
    let rate = reader.spec().sample_rate as f64;
    let samples = reader
        .samples::<i16>()
        .map(|s| s.map(|s| s as f32 / i16::MAX as f32))
        .collect::<std::result::Result<_, _>>()?;
    Ok((samples, rate))
}

fn sync_check(dir: PathBuf) -> Result<()> {
    let timing = parse_timing(&std::fs::read_to_string(dir.join("timing.jsonl"))?)?;
    let alignment = Alignment::fit(&timing)?;
    println!(
        "alignment: offset {:.2} ms, drift {:.1} ppm, worst residual {:.2} ms",
        alignment.offset_ms,
        (alignment.drift_ratio - 1.0) * 1e6,
        alignment.max_residual_ms
    );

    let (mic, mic_rate) = read_wav(dir.join("mic.wav"))?;
    let (system, system_rate) = read_wav(dir.join("system.wav"))?;
    let mic_clicks = find_clicks(&mic, mic_rate);
    let system_clicks: Vec<f64> = find_clicks(&system, system_rate)
        .into_iter()
        .map(|t| alignment.system_to_mic_ms(t))
        .collect();
    let offsets = click_offsets(&mic_clicks, &system_clicks, CLICK_MATCH_MS);
    println!(
        "clicks: {} on mic, {} on system, {} matched",
        mic_clicks.len(),
        system_clicks.len(),
        offsets.len()
    );
    for (i, offset) in offsets.iter().enumerate() {
        println!("  click {}: mic minus system {offset:.1} ms", i + 1);
    }
    match (offsets.first(), offsets.last()) {
        (Some(first), Some(last)) if offsets.len() > 1 => {
            println!("offset change, first to last: {:.1} ms", last - first);
            Ok(())
        }
        _ => Err("need at least 2 matched clicks to measure a change".into()),
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Record(args)) => record(args),
        Some(Command::SyncCheck { dir }) => sync_check(dir),
        None => {
            Cli::command().version(version()).print_help()?;
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let matches = Cli::command().version(version()).get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
