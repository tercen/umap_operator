//! Progress and log events for the task, so a long import is not a silent one.
//!
//! The R operator calls `ctx$progress(message, actual, total)` once per file and `ctx$log(...)`
//! for the conditions a user should know about. This is the equivalent.
//!
//! Two things shape the implementation:
//!
//! * `TercenLogger::progress` in tercen-rs takes a percent and then **drops it**, sending only the
//!   message, although `TaskProgressEvent` carries `actual` and `total` (`tercen_model.proto`).
//!   A bar with no numbers is not much better than silence, so the events are built here instead.
//! * The two long phases — the planning pass and the result write — are synchronous, and the
//!   planning pass is parallel. Reporting therefore goes through an unbounded channel whose
//!   `send` is non-blocking and callable from rayon workers; a background task drains it and does
//!   the gRPC round trips, so no phase ever waits on the network to report.
//!
//! Nothing here can fail an import: a dropped event is logged at debug and forgotten.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use tercen_rs::TercenClient;
use tercen_rs::client::proto::{self, EEvent, e_event};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

/// Percent bands per phase, so the bar only ever moves forwards.
pub const READ: (u8, u8) = (0, 35);
/// Training and metaclustering. It gets a wide band because the hundred consensus resamples
/// are the slow part on a large map, and a bar that sits still is a bar nobody trusts.
pub const FIT: (u8, u8) = (35, 75);
pub const WRITE: (u8, u8) = (75, 85);
pub const UPLOAD: (u8, u8) = (85, 100);

/// Map "item `i` of `n`" into a phase's band.
pub fn band(phase: (u8, u8), i: usize, n: usize) -> u8 {
    if n == 0 {
        return phase.1;
    }
    let (lo, hi) = (phase.0 as f64, phase.1 as f64);
    (lo + (hi - lo) * (i.min(n) as f64 / n as f64)).round() as u8
}

enum Msg {
    Progress { pct: u8, text: String },
    Log(String),
}

/// Sends progress and log events for one task. Cloneable and callable from synchronous code.
#[derive(Clone)]
pub struct Reporter {
    tx: Option<UnboundedSender<Msg>>,
    /// Last percent actually sent, so a phase that ticks far more often than the bar can move
    /// does not spend a gRPC round trip per tick. The upload reports per 1 MiB chunk, which is
    /// 7,700 ticks on a 7.7 GB result and only 30 meaningful ones.
    last: Arc<AtomicU8>,
}

impl Reporter {
    /// A reporter that sends nothing — dev runs (no task) and tests.
    pub fn silent() -> Self {
        Self {
            tx: None,
            last: Arc::new(AtomicU8::new(u8::MAX)),
        }
    }

    /// Spawn the forwarder for `task_id`. Events are sent in order, off the critical path.
    pub fn spawn(client: Arc<TercenClient>, task_id: String) -> Self {
        let (tx, mut rx) = unbounded_channel::<Msg>();
        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                let event = match m {
                    Msg::Progress { pct, text } => EEvent {
                        object: Some(e_event::Object::Taskprogressevent(
                            proto::TaskProgressEvent {
                                task_id: task_id.clone(),
                                message: text,
                                // R sends actual/total; the percent is carried the same way so the
                                // UI has numbers rather than only a string.
                                actual: pct as i32,
                                total: 100,
                                ..Default::default()
                            },
                        )),
                    },
                    Msg::Log(text) => EEvent {
                        object: Some(e_event::Object::Tasklogevent(proto::TaskLogEvent {
                            task_id: task_id.clone(),
                            message: text,
                            ..Default::default()
                        })),
                    },
                };
                match client.event_service() {
                    Ok(mut svc) => {
                        if let Err(e) = svc.create(tonic::Request::new(event)).await {
                            tracing::debug!("progress event dropped: {e}");
                        }
                    }
                    Err(e) => tracing::debug!("event service unavailable: {e}"),
                }
            }
        });
        Self {
            tx: Some(tx),
            last: Arc::new(AtomicU8::new(u8::MAX)),
        }
    }

    /// Report progress. Safe to call from synchronous and parallel code.
    pub fn at(&self, pct: u8, text: impl Into<String>) {
        if self.last.swap(pct, Ordering::Relaxed) == pct {
            return; // the bar cannot move; not worth an event or a log line
        }
        let text = text.into();
        tracing::info!(pct, "{text}");
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Progress { pct, text });
        }
    }

    /// A warning for the task log — the R operator's `ctx$log`. Use for things the user should
    /// know about the import, not for internal tracing.
    pub fn log(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.send_log(text);
    }

    /// A neutral note for the task log. Same channel as `log`, without implying a problem —
    /// and not subject to the percent throttle, so a completion line always lands.
    pub fn info(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::info!("{text}");
        self.send_log(text);
    }

    fn send_log(&self, text: String) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Log(text));
        }
    }
}

impl Default for Reporter {
    fn default() -> Self {
        Self::silent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_are_monotonic_and_stay_inside_their_phase() {
        for n in [1usize, 2, 7, 93, 1000] {
            let mut prev = 0;
            for i in 0..=n {
                let p = band(READ, i, n);
                assert!((READ.0..=READ.1).contains(&p), "n={n} i={i} -> {p}");
                assert!(p >= prev, "band went backwards: n={n} i={i}");
                prev = p;
            }
            assert_eq!(band(READ, n, n), READ.1);
        }
        // a zero-item phase reports complete rather than dividing by zero
        assert_eq!(band(UPLOAD, 0, 0), UPLOAD.1);
    }

    #[test]
    fn a_silent_reporter_accepts_everything() {
        let r = Reporter::silent();
        r.at(50, "halfway");
        r.log("something the user should know");
    }

    #[test]
    fn repeated_percents_are_collapsed() {
        // the upload ticks once per MiB; only percent changes should survive
        let r = Reporter::silent();
        let mut sent = 0;
        for i in 0..=1000usize {
            let pct = band(UPLOAD, i, 1000);
            let before = r.last.load(Ordering::Relaxed);
            r.at(pct, "uploading");
            if r.last.load(Ordering::Relaxed) != before {
                sent += 1;
            }
        }
        assert!(
            sent <= (UPLOAD.1 - UPLOAD.0) as usize + 1,
            "collapsed to {sent} events, expected at most one per percent"
        );
    }
}
