use crate::{
    config::{CameraConfiguration, ReopenPolicy},
    error::PublisherError,
};
use cameras::{CameraCapture, RawCapturedFrame, conversion::I420Frame};
use clap::Parser;
use std::{
    io::Write,
    process::Stdio,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, ChildStdout, Command},
    sync::watch,
};

const REOPEN_DELAY_CAP: Duration = Duration::from_secs(3);
/// How often a capture process that is waiting for a frame checks that its supervisor lives.
const SUPERVISOR_CHECK_INTERVAL: Duration = Duration::from_secs(1);

pub struct CapturedI420Frame {
    pub captured_at_unix_ns: u128,
    pub i420: I420Frame,
}

/// An isolated native reader: nokhwa's blocking reads cannot be cancelled.
/// Process ownership lets shutdown release even a stalled camera's native handles.
pub struct CaptureWorker {
    pub frames: watch::Receiver<Option<Arc<CapturedI420Frame>>>,
    child: Child,
    reader: tokio::task::JoinHandle<Result<(), PublisherError>>,
}

impl CaptureWorker {
    /// Start a capture subprocess using the calling executable's private capture entry point.
    ///
    /// # Errors
    /// Returns an error if the current executable or subprocess cannot be started.
    pub fn start(camera: &CameraConfiguration) -> Result<Self, PublisherError> {
        let (frame_sender, frames) = watch::channel(None);
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("__capture")
            .arg(camera.name())
            .arg(camera.device())
            .arg(std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        // Only the supervisor handles terminal signals and orders cleanup.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn()?;
        let output = child.stdout.take().ok_or_else(|| {
            PublisherError::Configuration("capture subprocess has no stdout pipe".to_owned())
        })?;
        let reader = tokio::spawn(async move {
            let result = read_frames(output, &frame_sender).await;
            frame_sender.send_replace(None);
            result
        });
        Ok(Self {
            frames,
            child,
            reader,
        })
    }

    /// Stop the capture subprocess and release its resources.
    ///
    /// # Errors
    /// Returns process, pipe or task failures observed during shutdown.
    pub async fn stop(mut self) -> Result<(), PublisherError> {
        let completed_status = self.child.try_wait()?;
        if completed_status.is_none() {
            self.child.kill().await?;
        }
        // Interrupt a partial pipe read after terminating the producer.
        self.reader.abort();
        match self.reader.await {
            Ok(result) if completed_status.is_some() => result?,
            Err(error) if !error.is_cancelled() => return Err(error.into()),
            _ => {}
        }
        if let Some(status) = completed_status
            && !status.success()
        {
            return Err(PublisherError::CaptureProcess(status));
        }
        Ok(())
    }
}

async fn read_frames(
    mut output: ChildStdout,
    frame_sender: &watch::Sender<Option<Arc<CapturedI420Frame>>>,
) -> Result<(), PublisherError> {
    loop {
        let width = match output.read_u32_le().await {
            Ok(width) => width,
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let height = output.read_u32_le().await?;
        let captured_at_unix_ns = output.read_u128_le().await?;
        let width_pixels =
            usize::try_from(width).map_err(|_| cameras::CameraError::InvalidFrame)?;
        let height_pixels =
            usize::try_from(height).map_err(|_| cameras::CameraError::InvalidFrame)?;
        let plane_length = width_pixels
            .checked_mul(height_pixels)
            .ok_or(cameras::CameraError::InvalidFrame)?;
        let chroma_length = width_pixels
            .div_ceil(2)
            .checked_mul(height_pixels.div_ceil(2))
            .ok_or(cameras::CameraError::InvalidFrame)?;
        let length = chroma_length
            .checked_mul(2)
            .and_then(|chroma| plane_length.checked_add(chroma))
            .ok_or(cameras::CameraError::InvalidFrame)?;
        let mut data = vec![0; length];
        output.read_exact(&mut data).await?;
        let i420 = I420Frame {
            width,
            height,
            data,
        };
        i420.planes()?;
        if frame_sender
            .send(Some(Arc::new(CapturedI420Frame {
                captured_at_unix_ns,
                i420,
            })))
            .is_err()
        {
            return Ok(());
        }
    }
}

#[derive(Parser)]
struct CaptureArguments {
    name: String,
    device: String,
    /// The process id of the supervisor that spawned this process.
    supervisor: u32,
}

/// Private subprocess entry point; stdout is a binary stream, diagnostics use stderr.
///
/// # Errors
/// Returns an error if initialization, capture or writing to the pipe fails.
pub fn run_capture_process() -> Result<(), PublisherError> {
    let arguments = CaptureArguments::parse_from(std::env::args_os().skip(1));
    let supervisor = arguments.supervisor;
    cameras::validate_camera_name(&arguments.name)?;
    cameras::initialize()?;
    let reopen_policy = ReopenPolicy::for_device(&arguments.device);
    let mut backoff = crate::backoff::Backoff::new(REOPEN_DELAY_CAP);
    let mut output = std::io::stdout().lock();
    loop {
        if supervisor_exited(supervisor) {
            return Ok(());
        }
        let attempt_started = std::time::Instant::now();
        match capture_to_pipe(&arguments.device, &mut output, supervisor) {
            Ok(()) => return Ok(()),
            Err(PublisherError::Io(error)) => return Err(error.into()),
            Err(error) => eprintln!("Camera {}: {error}", arguments.name),
        }
        match reopen_policy {
            ReopenPolicy::Stop => {
                eprintln!(
                    "Camera {}: index {} unavailable or disconnected; stopped because reopening may select another camera",
                    arguments.name, arguments.device
                );
                return Ok(());
            }
            ReopenPolicy::Retry => {
                let delay = backoff.next_delay(attempt_started.elapsed());
                eprintln!(
                    "Camera {}: reopening {} in {delay:?}",
                    arguments.name, arguments.device
                );
                std::thread::sleep(delay);
            }
        }
    }
}

/// Whether the supervisor that spawned this process is gone: the process is reparented when
/// it exits, even if that happened before this process first looked. A capture process owns a camera and is in its own process group, so it must notice
/// this itself; a device that cannot open writes nothing, so a broken pipe never shows it.
fn supervisor_exited(supervisor: u32) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::process::parent_id() != supervisor
    }
    // Without a portable parent query, a broken pipe is the only signal.
    #[cfg(not(unix))]
    {
        let _ = supervisor;
        false
    }
}

fn capture_to_pipe(
    device: &str,
    output: &mut impl Write,
    supervisor: u32,
) -> Result<(), PublisherError> {
    // Drain the native backend on its own thread. Some backends queue frames
    // without a bound; conversion or a slow pipe must not build up old images.
    let pending_capture = Arc::new(PendingCapture::<RawCapturedFrame>::default());
    let reader_capture = Arc::downgrade(&pending_capture);
    let device = device.to_owned();
    let capture_thread = std::thread::spawn(move || {
        let capture_result: Result<(), cameras::CameraError> = (|| {
            let mut capture = CameraCapture::open(&device)?;
            loop {
                let raw_frame = capture.read_raw_frame()?;
                let Some(pending_capture) = reader_capture.upgrade() else {
                    return Ok(());
                };
                pending_capture.replace(raw_frame)?;
            }
        })();
        if let Err(error) = capture_result
            && let Some(pending_capture) = reader_capture.upgrade()
        {
            let Ok(mut pending_state) = pending_capture.state.lock() else {
                return;
            };
            pending_state.error = Some(error);
            pending_capture.ready.notify_one();
        }
    });
    loop {
        let mut pending_state = pending_capture
            .state
            .lock()
            .map_err(|_| std::io::Error::other("camera capture state lock poisoned"))?;
        while pending_state.frame.is_none() && pending_state.error.is_none() {
            let (next_state, _) = pending_capture
                .ready
                .wait_timeout(pending_state, SUPERVISOR_CHECK_INTERVAL)
                .map_err(|_| std::io::Error::other("camera capture state lock poisoned"))?;
            pending_state = next_state;
            if capture_thread.is_finished()
                && pending_state.frame.is_none()
                && pending_state.error.is_none()
            {
                return Err(PublisherError::Configuration(
                    "camera reader stopped unexpectedly".to_owned(),
                ));
            }
            if supervisor_exited(supervisor) {
                return Ok(());
            }
        }
        if let Some(error) = pending_state.error.take() {
            return Err(error.into());
        }
        let raw_frame = pending_state
            .frame
            .take()
            .ok_or_else(|| std::io::Error::other("camera capture became ready without a frame"))?;
        drop(pending_state);
        let frame = raw_frame.into_video()?;
        // Same-executable private protocol: packed dimensions, Unix ns, then I420.
        output.write_all(&frame.i420.width.to_le_bytes())?;
        output.write_all(&frame.i420.height.to_le_bytes())?;
        output.write_all(&frame.captured_at_unix_ns.to_le_bytes())?;
        output.write_all(&frame.i420.data)?;
        output.flush()?;
    }
}

/// Only the newest unconverted frame is useful to a live subscriber.
struct PendingCapture<Frame> {
    state: Mutex<PendingCaptureState<Frame>>,
    ready: Condvar,
}

struct PendingCaptureState<Frame> {
    frame: Option<Frame>,
    error: Option<cameras::CameraError>,
}

impl<Frame> Default for PendingCapture<Frame> {
    fn default() -> Self {
        Self {
            state: Mutex::new(PendingCaptureState {
                frame: None,
                error: None,
            }),
            ready: Condvar::new(),
        }
    }
}

impl<Frame> PendingCapture<Frame> {
    fn replace(&self, frame: Frame) -> std::io::Result<()> {
        self.state
            .lock()
            .map_err(|_| std::io::Error::other("camera capture state lock poisoned"))?
            .frame = Some(frame);
        self.ready.notify_one();
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "Test assertions fail immediately on unexpected outcomes"
)]
mod tests {
    use super::PendingCapture;

    #[test]
    fn slow_conversion_resumes_with_the_newest_capture() {
        let pending_capture = PendingCapture::default();
        pending_capture.replace(1).unwrap();
        assert_eq!(pending_capture.state.lock().unwrap().frame.take(), Some(1));
        // Captures continue while conversion or the output pipe is busy.
        pending_capture.replace(2).unwrap();
        pending_capture.replace(3).unwrap();
        pending_capture.replace(4).unwrap();
        assert_eq!(pending_capture.state.lock().unwrap().frame.take(), Some(4));
    }
}
