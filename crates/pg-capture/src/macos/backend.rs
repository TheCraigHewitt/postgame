use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::shim::{Mic, SystemTap, audio_processes, own_process_object};
use crate::target::{SystemTarget, TapPlan, plan_tap, tap_is_stale};
use crate::track::{Buffer, HealthLog, JsonlLog, TrackSummary, TrackWriter};
use crate::{CaptureBackend, CaptureError, CaptureSummary};

const SUPERVISOR_TICK: Duration = Duration::from_millis(250);

struct Running {
    mic: Mic,
    stop: Arc<AtomicBool>,
    supervisor: JoinHandle<()>,
    mic_writer: JoinHandle<TrackSummary>,
    system_writer: JoinHandle<TrackSummary>,
    health: HealthLog,
}

/// Records `mic.wav`, `system.wav`, `timing.jsonl` and `health.jsonl` to a directory.
pub struct MacCapture {
    out_dir: PathBuf,
    running: Option<Running>,
}

impl MacCapture {
    pub fn new(out_dir: PathBuf) -> Self {
        Self {
            out_dir,
            running: None,
        }
    }
}

fn spawn_writer(
    mut writer: TrackWriter,
    buffers: Receiver<Buffer>,
    health: HealthLog,
) -> JoinHandle<TrackSummary> {
    thread::spawn(move || {
        let mut failed = false;
        for buffer in buffers {
            if let Err(e) = writer.write(&buffer)
                && !std::mem::replace(&mut failed, true)
            {
                health.event("write_failed", &e.to_string());
            }
        }
        writer.finish().unwrap_or_else(|e| {
            health.event("finalize_failed", &e.to_string());
            TrackSummary::default()
        })
    })
}

/// Owns the system tap: starts it when the target appears, rebuilds it when a
/// new helper starts outputting, and falls back to a global tap.
struct Supervisor {
    target: SystemTarget,
    sender: Sender<Buffer>,
    health: HealthLog,
    started: Instant,
    tap: Option<SystemTap>,
    tapped: Vec<u32>,
    gave_up: bool,
}

impl Supervisor {
    fn tick(&mut self) {
        if self.gave_up {
            return;
        }
        let processes = audio_processes();
        if self.tap.is_some() {
            if !self.tapped.is_empty() && tap_is_stale(&processes, &self.target, &self.tapped) {
                self.tap = None;
                self.apply(plan_tap(&processes, &self.target, self.started.elapsed()));
            }
            return;
        }
        self.apply(plan_tap(&processes, &self.target, self.started.elapsed()));
    }

    fn apply(&mut self, plan: TapPlan) {
        let (ids, exclude, detail) = match plan {
            TapPlan::Wait => return,
            TapPlan::Processes(ids) => {
                let detail = format!("tapping {} process(es) of {:?}", ids.len(), self.target);
                (ids, false, detail)
            }
            TapPlan::Global => {
                let excluded = own_process_object().into_iter().collect();
                let detail = match self.target {
                    SystemTarget::AllSystem => "global tap requested".to_string(),
                    _ => format!(
                        "no output from {:?}: fell back to a global tap",
                        self.target
                    ),
                };
                (excluded, true, detail)
            }
        };
        let health = self.health.clone();
        let on_extra_buffers = Box::new(move |count: u32| {
            let detail =
                format!("the tap delivered {count} buffers, so a device's input may be mixed in");
            health.event("unexpected_tap_buffers", &detail);
        });
        match SystemTap::start(&ids, exclude, self.sender.clone(), on_extra_buffers) {
            Ok(tap) => {
                self.health.event("tap_started", &detail);
                self.tap = Some(tap);
                self.tapped = if exclude { Vec::new() } else { ids };
            }
            Err(e) => {
                self.health.event("tap_failed", &e.to_string());
                eprintln!("system audio capture failed, continuing mic-only: {e}");
                self.gave_up = true;
            }
        }
    }
}

impl CaptureBackend for MacCapture {
    fn start(&mut self, target: SystemTarget) -> Result<(), CaptureError> {
        if self.running.is_some() {
            return Err(CaptureError::Start("already recording".into()));
        }
        std::fs::create_dir_all(&self.out_dir)?;
        if self.out_dir.join("mic.wav").exists() {
            return Err(CaptureError::Start(format!(
                "{} already holds a recording",
                self.out_dir.display()
            )));
        }
        let health = HealthLog::create(&self.out_dir.join("health.jsonl"))?;
        let timing = JsonlLog::create(&self.out_dir.join("timing.jsonl"))?;

        let writer = |name, file: &str| {
            TrackWriter::new(
                name,
                self.out_dir.join(file),
                timing.clone(),
                health.clone(),
            )
        };
        let (mic_sender, mic_buffers) = channel();
        let (system_sender, system_buffers) = channel();
        let mic_writer = spawn_writer(writer("mic", "mic.wav"), mic_buffers, health.clone());
        let system_writer = spawn_writer(
            writer("system", "system.wav"),
            system_buffers,
            health.clone(),
        );

        let mic = Mic::start(mic_sender)?;
        health.event("mic_started", "default input device");

        let stop = Arc::new(AtomicBool::new(false));
        let mut supervisor = Supervisor {
            target,
            sender: system_sender,
            health: health.clone(),
            started: Instant::now(),
            tap: None,
            tapped: Vec::new(),
            gave_up: false,
        };
        let stop_flag = stop.clone();
        let supervisor = thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                supervisor.tick();
                thread::sleep(SUPERVISOR_TICK);
            }
        });

        self.running = Some(Running {
            mic,
            stop,
            supervisor,
            mic_writer,
            system_writer,
            health,
        });
        Ok(())
    }

    fn stop(&mut self) -> Result<CaptureSummary, CaptureError> {
        let running = self.running.take().ok_or(CaptureError::NotRecording)?;
        running.stop.store(true, Ordering::Relaxed);
        let lost = |_| CaptureError::WriterPanicked;
        running.supervisor.join().map_err(lost)?;
        drop(running.mic);
        let summary = CaptureSummary {
            mic: running.mic_writer.join().map_err(lost)?,
            system: running.system_writer.join().map_err(lost)?,
        };
        running.health.event("stopped", "");
        Ok(summary)
    }
}
