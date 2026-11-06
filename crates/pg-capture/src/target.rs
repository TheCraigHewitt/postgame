//! Decides which processes the system track should tap.

use std::time::Duration;

use serde::Deserialize;

/// What the system track records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemTarget {
    /// Every process whose bundle ID starts with this prefix, helpers included.
    BundlePrefix(String),
    /// One process. It never falls back to a global tap.
    Pid(i32),
    /// All system audio except our own process.
    AllSystem,
}

/// A process known to Core Audio.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AudioProcess {
    pub object_id: u32,
    pub pid: i32,
    pub bundle_id: String,
    pub running_output: bool,
}

/// How long a bundle target may stay silent before we tap everything.
pub const FALLBACK_AFTER: Duration = Duration::from_secs(10);

/// The tap a recording should have right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TapPlan {
    Wait,
    Processes(Vec<u32>),
    Global,
}

fn matches(process: &AudioProcess, target: &SystemTarget) -> bool {
    match target {
        SystemTarget::BundlePrefix(prefix) => process.bundle_id.starts_with(prefix.as_str()),
        SystemTarget::Pid(pid) => process.pid == *pid,
        SystemTarget::AllSystem => false,
    }
}

/// Picks the tap for `target`. Once any matching process is outputting, every
/// matching process is included, because browsers play audio from helpers.
pub fn plan_tap(processes: &[AudioProcess], target: &SystemTarget, elapsed: Duration) -> TapPlan {
    if *target == SystemTarget::AllSystem {
        return TapPlan::Global;
    }
    let matching: Vec<&AudioProcess> = processes.iter().filter(|p| matches(p, target)).collect();
    if matching.iter().any(|p| p.running_output) {
        return TapPlan::Processes(matching.iter().map(|p| p.object_id).collect());
    }
    match target {
        SystemTarget::BundlePrefix(_) if elapsed >= FALLBACK_AFTER => TapPlan::Global,
        _ => TapPlan::Wait,
    }
}

/// True if a process matching `target` is outputting but missing from `tapped`.
pub fn tap_is_stale(processes: &[AudioProcess], target: &SystemTarget, tapped: &[u32]) -> bool {
    processes
        .iter()
        .any(|p| matches(p, target) && p.running_output && !tapped.contains(&p.object_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(object_id: u32, bundle_id: &str, running_output: bool) -> AudioProcess {
        AudioProcess {
            object_id,
            pid: object_id as i32 + 1000,
            bundle_id: bundle_id.into(),
            running_output,
        }
    }

    fn chrome() -> SystemTarget {
        SystemTarget::BundlePrefix("com.google.Chrome".into())
    }

    const EARLY: Duration = Duration::from_secs(2);

    #[test]
    fn helpers_are_included_once_any_of_them_outputs() {
        let processes = [
            process(1, "com.google.Chrome", false),
            process(2, "com.google.Chrome.helper", true),
            process(3, "us.zoom.xos", true),
        ];
        assert_eq!(
            plan_tap(&processes, &chrome(), EARLY),
            TapPlan::Processes(vec![1, 2])
        );
    }

    #[test]
    fn waits_while_nothing_matching_outputs() {
        let processes = [process(1, "com.google.Chrome", false)];
        assert_eq!(plan_tap(&processes, &chrome(), EARLY), TapPlan::Wait);
        assert_eq!(plan_tap(&[], &chrome(), EARLY), TapPlan::Wait);
    }

    #[test]
    fn falls_back_to_global_at_ten_seconds() {
        let almost = FALLBACK_AFTER - Duration::from_millis(1);
        assert_eq!(plan_tap(&[], &chrome(), almost), TapPlan::Wait);
        assert_eq!(plan_tap(&[], &chrome(), FALLBACK_AFTER), TapPlan::Global);
    }

    #[test]
    fn a_pid_target_never_falls_back() {
        let hour = Duration::from_secs(3600);
        assert_eq!(plan_tap(&[], &SystemTarget::Pid(7), hour), TapPlan::Wait);
    }

    #[test]
    fn a_pid_target_matches_only_that_process() {
        let processes = [process(1, "", true), process(2, "", true)];
        assert_eq!(
            plan_tap(&processes, &SystemTarget::Pid(1002), EARLY),
            TapPlan::Processes(vec![2])
        );
    }

    #[test]
    fn all_system_is_global_immediately() {
        assert_eq!(
            plan_tap(&[], &SystemTarget::AllSystem, Duration::ZERO),
            TapPlan::Global
        );
    }

    #[test]
    fn a_new_outputting_helper_makes_the_tap_stale() {
        let processes = [
            process(1, "com.google.Chrome", true),
            process(2, "com.google.Chrome.helper", true),
        ];
        assert!(tap_is_stale(&processes, &chrome(), &[1]));
        assert!(!tap_is_stale(&processes, &chrome(), &[1, 2]));
    }

    #[test]
    fn a_new_idle_or_unrelated_process_does_not_make_the_tap_stale() {
        let processes = [
            process(1, "com.google.Chrome", true),
            process(2, "com.google.Chrome.helper", false),
            process(3, "us.zoom.xos", true),
        ];
        assert!(!tap_is_stale(&processes, &chrome(), &[1]));
    }

    #[test]
    fn process_json_from_the_shim_parses() {
        let json = r#"[{"object_id":5,"pid":9,"bundle_id":"a.b","running_output":true}]"#;
        let parsed: Vec<AudioProcess> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed[0].bundle_id, "a.b");
    }
}
