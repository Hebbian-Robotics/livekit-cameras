use crate::{
    config::{CameraConfiguration, ReopenPolicy},
    error::PublisherError,
};
use cameras::{CameraCapture, conversion::I420Frame};
use clap::Parser;
use std::{io::Write, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::{Child, ChildStdout, Command},
    sync::watch,
};

const REOPEN_DELAY_CAP: Duration = Duration::from_secs(3);

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
}

/// Private subprocess entry point; stdout is a binary stream, diagnostics use stderr.
///
/// # Errors
/// Returns an error if initialization, capture or writing to the pipe fails.
pub fn run_capture_process() -> Result<(), PublisherError> {
    let arguments = CaptureArguments::parse_from(std::env::args_os().skip(1));
    cameras::validate_camera_name(&arguments.name)?;
    cameras::initialize()?;
    let reopen_policy = ReopenPolicy::for_device(&arguments.device);
    let mut backoff = crate::backoff::Backoff::new(REOPEN_DELAY_CAP);
    let mut output = std::io::stdout().lock();
    loop {
        let attempt_started = std::time::Instant::now();
        match capture_to_pipe(&arguments.device, &mut output) {
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

fn capture_to_pipe(device: &str, output: &mut impl Write) -> Result<(), PublisherError> {
    // Drain the native backend on its own thread. Some backends queue frames
    // without a bound; conversion or a slow pipe must not build up old images.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let device = device.to_owned();
    std::thread::spawn(move || {
        let capture_result: Result<(), cameras::CameraError> = (|| {
            let mut capture = CameraCapture::open(&device)?;
            loop {
                let raw_frame = capture.read_raw_frame()?;
                match sender.try_send(Ok(raw_frame)) {
                    Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return Ok(()),
                }
            }
        })();
        if let Err(error) = capture_result {
            let _ = sender.send(Err(error));
        }
    });
    loop {
        let raw_frame = receiver.recv().map_err(|_| {
            PublisherError::Configuration("camera reader stopped unexpectedly".to_owned())
        })??;
        let frame = raw_frame.into_video()?;
        // Same-executable private protocol: packed dimensions, Unix ns, then I420.
        output.write_all(&frame.i420.width.to_le_bytes())?;
        output.write_all(&frame.i420.height.to_le_bytes())?;
        output.write_all(&frame.captured_at_unix_ns.to_le_bytes())?;
        output.write_all(&frame.i420.data)?;
        output.flush()?;
    }
}
