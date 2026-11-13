use super::{
    RecorderError, VideoEvent, VideoOutput,
    nut::{NutReader, NutWriter},
    unix_nanoseconds,
};
use cameras::conversion::I420Frame;
use livekit::webrtc::video_frame::VideoRotation;
#[cfg(unix)]
use nix::sys::signal::{Signal, kill};
#[cfg(unix)]
use nix::unistd::Pid;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        mpsc::{Receiver, RecvTimeoutError},
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PictureFormat {
    pub width: u32,
    pub height: u32,
    pub rotation: livekit::webrtc::video_frame::VideoRotation,
}

impl PictureFormat {
    fn is_sideways(self) -> bool {
        matches!(
            self.rotation,
            VideoRotation::VideoRotation90 | VideoRotation::VideoRotation270
        )
    }

    /// Size of the video after the encoder rotates it upright.
    fn encoded_width(self) -> u32 {
        if self.is_sideways() {
            self.height
        } else {
            self.width
        }
    }

    fn encoded_height(self) -> u32 {
        if self.is_sideways() {
            self.width
        } else {
            self.height
        }
    }
}

/// Owns `FFmpeg` even on a failed pipe write or mux operation. A thread blocks
/// in `wait` and reports the exit, so waiting is an event, not polling.
struct EncoderProcess {
    process_id: u32,
    exited: Receiver<std::io::Result<ExitStatus>>,
    reaped: bool,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
}

impl EncoderProcess {
    fn new(mut child: Child) -> Self {
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let process_id = child.id();
        let (sender, exited) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(child.wait());
        });
        Self {
            process_id,
            exited,
            reaped: false,
            stdin,
            stdout,
        }
    }

    /// The exit status, or `None` when the process outlives `limit`.
    fn exit_within(&mut self, limit: Duration) -> std::io::Result<Option<ExitStatus>> {
        match self.exited.recv_timeout(limit) {
            Ok(status) => {
                self.reaped = true;
                status.map(Some)
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                self.reaped = true;
                Err(std::io::Error::other("the process waiter ended"))
            }
        }
    }

    fn wait(&mut self, operation: &'static str) -> Result<(), RecorderError> {
        match self.exit_within(Duration::from_secs(30))? {
            Some(status) if status.success() => Ok(()),
            Some(status) => Err(RecorderError::Process { operation, status }),
            None => Err(RecorderError::Timeout(operation)),
        }
    }
}

impl Drop for EncoderProcess {
    fn drop(&mut self) {
        if !self.reaped
            && let Ok(pid) = i32::try_from(self.process_id)
        {
            #[cfg(unix)]
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
            #[cfg(windows)]
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .status();
            let _ = self.exited.recv();
        }
    }
}

#[cfg(target_os = "linux")]
#[must_use]
pub fn hardware_encoder_available() -> bool {
    let process = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-vaapi_device",
            "/dev/dri/renderD128",
            "-f",
            "lavfi",
            "-i",
            "color=size=640x480:rate=15",
            "-frames:v",
            "1",
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            "h264_vaapi",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(child) = process else { return false };
    let mut process = EncoderProcess::new(child);
    matches!(process.exit_within(Duration::from_secs(10)), Ok(Some(status)) if status.success())
}

#[cfg(not(target_os = "linux"))]
#[must_use]
pub fn hardware_encoder_available() -> bool {
    false
}

/// Encodes raw frames with `FFmpeg` and passes the H.264 packets to the output.
struct TimestampedEncoder {
    pipe: Option<ChildStdin>,
    frames: NutWriter,
    process: EncoderProcess,
    packets: Option<std::thread::JoinHandle<Result<u64, RecorderError>>>,
    first_at_unix_us: u64,
    last_pts: i64,
}

impl TimestampedEncoder {
    fn start(
        picture: PictureFormat,
        hardware: bool,
        first_at: u64,
        camera: &str,
        archive: &Arc<dyn VideoOutput>,
    ) -> Result<Self, RecorderError> {
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error"]);
        if hardware {
            command.args(["-vaapi_device", "/dev/dri/renderD128"]);
        }
        command.args(["-f", "nut", "-i", "pipe:0"]);
        let rotation = match picture.rotation {
            VideoRotation::VideoRotation0 => "",
            VideoRotation::VideoRotation90 => "transpose=clock",
            VideoRotation::VideoRotation180 => "transpose=clock,transpose=clock",
            VideoRotation::VideoRotation270 => "transpose=cclock",
        };
        let filter = [rotation, if hardware { "format=nv12,hwupload" } else { "" }]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if !filter.is_empty() {
            command.args(["-vf", &filter]);
        }
        // Preserve source timestamps with a portable microsecond time base.
        command.args(["-fps_mode", "vfr", "-enc_time_base", "1:1000000"]);
        if hardware {
            command.args(["-c:v", "h264_vaapi", "-qp", "23"]);
        } else {
            command.args([
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-tune",
                "zerolatency",
                "-crf",
                "23",
                "-pix_fmt",
                "yuv420p",
            ]);
        }
        // Every key frame carries its SPS/PPS, as the MCAP's Annex B video needs.
        command
            .args(["-bf", "0", "-g", "30", "-bsf:v", "dump_extra=freq=keyframe"])
            .args(["-f", "nut", "pipe:1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(unix)]
        command.process_group(0);
        let mut process = EncoderProcess::new(command.spawn()?);
        let pipe = process
            .stdin
            .take()
            .ok_or_else(|| RecorderError::InvalidData("encoder stdin unavailable".into()))?;
        let encoded = process
            .stdout
            .take()
            .ok_or_else(|| RecorderError::InvalidData("encoder stdout unavailable".into()))?;
        let first_at_unix_us = first_at / 1000;
        let archive = archive.clone();
        let camera = camera.to_owned();
        let packets = std::thread::spawn(move || {
            archive_packets(encoded, &camera, first_at_unix_us, &archive)
        });
        Ok(Self {
            pipe: Some(pipe),
            frames: NutWriter::new(picture.width, picture.height),
            process,
            packets: Some(packets),
            first_at_unix_us,
            last_pts: -1,
        })
    }

    fn write(&mut self, frame: &I420Frame, timestamp: u64) -> Result<(), RecorderError> {
        frame.planes()?;
        let relative = i128::from(timestamp / 1000) - i128::from(self.first_at_unix_us);
        let pts = i64::try_from(relative.max(i128::from(self.last_pts) + 1))
            .map_err(|_| RecorderError::InvalidData("frame timestamp overflow".into()))?;
        self.last_pts = pts;
        let pipe = self
            .pipe
            .as_mut()
            .ok_or_else(|| RecorderError::InvalidData("encoder already finished".into()))?;
        self.frames.write_frame(pipe, pts, &frame.data)?;
        Ok(())
    }

    /// Finish the encode; returns how many packets reached the MCAP and when the
    /// last frame was captured.
    fn finish(mut self) -> Result<FinishedEncode, RecorderError> {
        drop(self.pipe.take());
        self.process.wait("video encoder")?;
        let packets = self
            .packets
            .take()
            .ok_or_else(|| RecorderError::InvalidData("packet reader already joined".into()))?
            .join()
            .map_err(|_| RecorderError::InvalidData("packet reader panicked".into()))??;
        let last_frame_unix_nano = u64::try_from(
            i128::from(self.first_at_unix_us) * 1000 + i128::from(self.last_pts.max(0)) * 1000,
        )
        .map_err(|_| RecorderError::InvalidData("frame timestamp overflow".into()))?;
        Ok(FinishedEncode {
            packets,
            last_frame_unix_nano,
        })
    }
}

struct FinishedEncode {
    packets: u64,
    last_frame_unix_nano: u64,
}

/// Copy each encoded packet into the MCAP at its capture time.
fn archive_packets(
    encoded: ChildStdout,
    camera: &str,
    first_at_unix_us: u64,
    archive: &Arc<dyn VideoOutput>,
) -> Result<u64, RecorderError> {
    let mut reader = NutReader::new(encoded);
    let mut packets = 0_u64;
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => return Ok(packets),
            // An encoder killed mid-write ends the stream abruptly; keep what arrived.
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(packets),
            Err(error) => return Err(error.into()),
        };
        let (numerator, denominator) = reader.time_base();
        let elapsed = i128::from(packet.timestamp) * i128::from(numerator) * 1_000_000_000
            / i128::from(denominator.max(1));
        let timestamp = u64::try_from(i128::from(first_at_unix_us) * 1000 + elapsed)
            .map_err(|_| RecorderError::InvalidData("encoded timestamp overflow".into()))?;
        archive.video(camera, timestamp, packet.data)?;
        packets += 1;
    }
}

pub struct CameraWriter {
    camera: String,
    hardware: bool,
    archive: Arc<dyn VideoOutput>,
    segment: Option<Segment>,
}

struct Segment {
    picture: PictureFormat,
    /// The camera and the segment's start time.
    name: String,
    encoder: TimestampedEncoder,
    received_frames: u64,
}

impl CameraWriter {
    /// Create an encoder for a named camera.
    ///
    /// # Errors
    /// Returns an error if the camera name is invalid.
    pub fn new(
        camera: String,
        hardware: bool,
        archive: Arc<dyn VideoOutput>,
    ) -> Result<Self, RecorderError> {
        cameras::validate_camera_name(&camera)?;
        Ok(Self {
            camera,
            hardware,
            archive,
            segment: None,
        })
    }

    /// Encode a frame at its Unix timestamp in nanoseconds.
    ///
    /// # Errors
    /// Returns an encoder or output error.
    pub fn write(
        &mut self,
        frame: &I420Frame,
        rotation: livekit::webrtc::video_frame::VideoRotation,
        timestamp: u64,
    ) -> Result<(), RecorderError> {
        let picture = PictureFormat {
            width: frame.width,
            height: frame.height,
            rotation,
        };
        if self
            .segment
            .as_ref()
            .is_none_or(|segment| segment.picture != picture)
        {
            self.finish_segment()?;
            let seconds = i64::try_from(timestamp / 1_000_000_000)
                .map_err(|_| RecorderError::InvalidData("timestamp overflow".into()))?;
            let nanos = u32::try_from(timestamp % 1_000_000_000)
                .map_err(|_| RecorderError::InvalidData("nanosecond overflow".into()))?;
            let started = chrono::DateTime::from_timestamp(seconds, nanos)
                .ok_or_else(|| RecorderError::InvalidData("timestamp outside UTC range".into()))?;
            let encoder = TimestampedEncoder::start(
                picture,
                self.hardware,
                timestamp,
                &self.camera,
                &self.archive,
            )?;
            let segment = format!(
                "{}/{}",
                self.camera,
                started.format("%Y-%m-%dT%H-%M-%S-%6fZ")
            );
            self.archive.event(VideoEvent::SegmentStarted {
                camera: &self.camera,
                segment: &segment,
                timestamp,
                width: picture.encoded_width(),
                height: picture.encoded_height(),
            })?;
            self.segment = Some(Segment {
                picture,
                name: segment,
                encoder,
                received_frames: 0,
            });
        }
        if let Some(segment) = &mut self.segment {
            segment.encoder.write(frame, timestamp)?;
            segment.received_frames += 1;
        }
        Ok(())
    }

    /// Finalize the active segment.
    ///
    /// # Errors
    /// Returns an encoder or output error.
    pub fn finish(&mut self) -> Result<(), RecorderError> {
        self.finish_segment()
    }

    fn finish_segment(&mut self) -> Result<(), RecorderError> {
        if let Some(segment) = self.segment.take() {
            let finished = segment.encoder.finish()?;
            self.archive.event(VideoEvent::SegmentEnded {
                camera: &self.camera,
                segment: &segment.name,
                timestamp: finished.last_frame_unix_nano,
            })?;
            self.archive.event(VideoEvent::Statistics {
                camera: &self.camera,
                received: segment.received_frames,
                encoded: finished.packets,
                timestamp: unix_nanoseconds(),
            })?;
        }
        Ok(())
    }
}
