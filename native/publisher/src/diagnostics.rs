use livekit::{
    prelude::LocalVideoTrack,
    webrtc::stats::{OutboundRtpStats, RtcStats},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::Instant;
use tokio_util::task::AbortOnDropHandle;

/// The task is aborted with its publication, including on errors or cancellation.
pub(crate) struct PublishingDiagnostics {
    _task: AbortOnDropHandle<()>,
    submission_age_us: Option<Arc<AtomicU64>>,
}

impl PublishingDiagnostics {
    pub(crate) fn start(name: String, track: LocalVideoTrack, enabled: bool) -> Self {
        let submission_age_us = enabled.then(|| Arc::new(AtomicU64::new(u64::MAX)));
        let reported_age = submission_age_us.clone();
        let task = tokio::spawn(async move {
            // The encoder may not exist until the first frame has been submitted.
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut previous = None;
            loop {
                match track.get_stats().await {
                    Ok(statistics) => {
                        report_statistics(
                            &name,
                            &statistics,
                            reported_age.as_deref(),
                            &mut previous,
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "Camera publisher {name}: publishing statistics unavailable: {error}"
                        );
                    }
                }
                if !enabled {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        Self {
            _task: AbortOnDropHandle::new(task),
            submission_age_us,
        }
    }

    pub(crate) fn record_submission(&self, captured_at_unix_ns: u128) {
        let Some(age) = &self.submission_age_us else {
            return;
        };
        let microseconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|now| now.as_nanos().checked_sub(captured_at_unix_ns))
            .and_then(|nanoseconds| u64::try_from(nanoseconds / 1000).ok())
            .unwrap_or(u64::MAX);
        age.store(microseconds, Ordering::Relaxed);
    }
}

fn report_statistics(
    name: &str,
    statistics: &[RtcStats],
    submission_age_us: Option<&AtomicU64>,
    previous: &mut Option<EncodingSnapshot>,
) {
    let mut found_video = false;
    for statistic in statistics {
        let RtcStats::OutboundRtp(outbound) = statistic else {
            continue;
        };
        if outbound.stream.kind != "video" {
            continue;
        }
        let codec = statistics
            .iter()
            .find_map(|statistic| match statistic {
                RtcStats::Codec(codec) if codec.rtc.id == outbound.stream.codec_id => {
                    Some(codec.codec.mime_type.as_str())
                }
                _ => None,
            })
            .unwrap_or("unavailable");
        // Retransmission streams do not describe the video encoder.
        if codec.eq_ignore_ascii_case("video/rtx") {
            continue;
        }
        found_video = true;
        let encoder = if outbound.outbound.encoder_implementation.is_empty() {
            "unavailable"
        } else {
            &outbound.outbound.encoder_implementation
        };
        if previous.is_none() {
            eprintln!("Camera publisher {name}: negotiated codec={codec}; encoder={encoder}");
        }
        if let Some(age) = submission_age_us {
            let current = EncodingSnapshot::new(outbound);
            if let Some(previous) = previous.as_ref() {
                if let Some(rates) = current.rates_since(previous) {
                    let age = match age.load(Ordering::Relaxed) {
                        u64::MAX => "unavailable".to_owned(),
                        microseconds => {
                            format!(
                                "{:.2} ms",
                                Duration::from_micros(microseconds).as_secs_f64() * 1000.0
                            )
                        }
                    };
                    let encode_time = rates.encode_ms.map_or_else(
                        || "unavailable".to_owned(),
                        |milliseconds| format!("{milliseconds:.2} ms"),
                    );
                    eprintln!(
                        "Camera publisher {name}: codec={codec}; encoder={encoder}; encoded_fps={:.2}; sent_kbit_s={:.1}; mean_encode={encode_time}; latest_submission_age={age}",
                        rates.frames_per_second, rates.kilobits_per_second
                    );
                } else {
                    eprintln!(
                        "Camera publisher {name}: publishing counters reset; collecting a new interval"
                    );
                }
            }
            *previous = Some(current);
        }
    }
    if !found_video {
        eprintln!("Camera publisher {name}: codec and encoder statistics unavailable");
    }
}

struct EncodingSnapshot {
    sampled_at: Instant,
    ssrc: u32,
    frames: u32,
    bytes: u64,
    encode_seconds: f64,
}

impl EncodingSnapshot {
    fn new(statistics: &OutboundRtpStats) -> Self {
        Self {
            sampled_at: Instant::now(),
            ssrc: statistics.stream.ssrc,
            frames: statistics.outbound.frames_encoded,
            bytes: statistics.sent.bytes_sent,
            encode_seconds: statistics.outbound.total_encode_time,
        }
    }

    // Throughput is approximate telemetry; sub-byte precision is unnecessary.
    #[allow(clippy::cast_precision_loss)]
    fn rates_since(&self, previous: &Self) -> Option<EncodingRates> {
        if self.ssrc != previous.ssrc {
            return None;
        }
        let seconds = self
            .sampled_at
            .checked_duration_since(previous.sampled_at)?
            .as_secs_f64();
        let frames = self.frames.checked_sub(previous.frames)?;
        let bytes = self.bytes.checked_sub(previous.bytes)?;
        let encode_seconds = self.encode_seconds - previous.encode_seconds;
        if seconds <= 0.0 || !encode_seconds.is_finite() || encode_seconds < 0.0 {
            return None;
        }
        Some(EncodingRates {
            frames_per_second: f64::from(frames) / seconds,
            kilobits_per_second: bytes as f64 * 8.0 / seconds / 1000.0,
            encode_ms: (frames > 0).then(|| encode_seconds * 1000.0 / f64::from(frames)),
        })
    }
}

struct EncodingRates {
    frames_per_second: f64,
    kilobits_per_second: f64,
    encode_ms: Option<f64>,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "Test assertions fail immediately on unexpected outcomes"
)]
mod tests {
    use super::*;

    #[test]
    fn rates_use_interval_deltas_and_reject_reset_counters() {
        let previous = EncodingSnapshot {
            sampled_at: Instant::now(),
            ssrc: 1,
            frames: 100,
            bytes: 50_000,
            encode_seconds: 1.0,
        };
        let mut current = EncodingSnapshot {
            sampled_at: previous.sampled_at + Duration::from_secs(5),
            ssrc: 1,
            frames: 175,
            bytes: 675_000,
            encode_seconds: 1.15,
        };
        let rates = current.rates_since(&previous).unwrap();
        assert!((rates.frames_per_second - 15.0).abs() < 1e-9);
        assert!((rates.kilobits_per_second - 1000.0).abs() < 1e-9);
        assert!((rates.encode_ms.unwrap() - 2.0).abs() < 1e-9);
        current.frames = previous.frames;
        assert!(current.rates_since(&previous).unwrap().encode_ms.is_none());
        current.frames = 0;
        assert!(current.rates_since(&previous).is_none());
        current.frames = 175;
        current.ssrc = 2;
        assert!(current.rates_since(&previous).is_none());
    }
}
