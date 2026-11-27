//! Background streaming tasks (`scan`, `verify`, `doctor`) and shared progress DTOs.

pub(crate) mod checks;
pub(crate) mod scan;

use std::time::{Duration, Instant};

use serde::Serialize;

/// Minimum interval between incremental progress / hit batch events sent to the webview.
pub(crate) const HIT_BATCH_INTERVAL: Duration = Duration::from_millis(300);

/// Shared progress envelope for `verify://progress` and `doctor://progress` events.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskProgressDto {
    pub(crate) phase: &'static str,
    pub(crate) done: usize,
    pub(crate) total: usize,
    /// `None` wherever there is no honest denominator (e.g. `prepare` or empty store).
    pub(crate) percent: Option<f64>,
    pub(crate) findings: usize,
}

/// Shared acknowledgement for `*_start` commands.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskStartDto {
    pub(crate) started: bool,
    pub(crate) running: bool,
}

/// Shared acknowledgement for `*_cancel` commands.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskCancelDto {
    pub(crate) canceling: bool,
}

/// Percentage finished rounded to one decimal place, or `None` when `total == 0`.
pub(crate) fn tick_percent(done: usize, total: usize) -> Option<f64> {
    (total > 0).then(|| ((done as f64 / total as f64 * 100.0) * 10.0).round() / 10.0)
}

/// Rate-limiter for progress ticks: emits immediately on phase change or after [`HIT_BATCH_INTERVAL`].
#[derive(Default)]
pub(crate) struct TickForward {
    phase: Option<&'static str>,
    since_emit: Option<Instant>,
}

impl TickForward {
    pub(crate) fn due(&mut self, phase: &'static str) -> bool {
        let changed = self.phase != Some(phase);
        self.phase = Some(phase);
        let elapsed = self
            .since_emit
            .map(|t| t.elapsed() >= HIT_BATCH_INTERVAL)
            .unwrap_or(true);
        if changed || elapsed {
            self.since_emit = Some(Instant::now());
            return true;
        }
        false
    }
}
