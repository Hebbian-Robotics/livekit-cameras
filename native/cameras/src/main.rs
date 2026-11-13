use cameras::{CameraCapture, CameraError, config::configure_camera};
use clap::{Parser, Subcommand};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(about = "Native camera capture to JPEG and I420")]
struct Arguments {
    /// Camera mapping, repeatable.
    #[arg(long = "camera", global = true)]
    camera_mappings: Vec<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enumerate native devices and configured logical names.
    List,
    /// Capture N frames, writing the final JPEG to --out.
    Grab {
        /// Configured logical name, native index, Linux path or macOS device identifier.
        name: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        count: u32,
    },
}

fn run() -> Result<(), CameraError> {
    let arguments = Arguments::parse();
    let mut configured_cameras = BTreeMap::new();
    for specification in arguments.camera_mappings {
        configure_camera(&mut configured_cameras, &specification)?;
    }
    cameras::initialize()?;
    match arguments.command {
        Command::List => {
            for (name, device) in configured_cameras {
                println!("configured\t{name}\t{device}");
            }
            for camera in cameras::list_cameras()? {
                println!(
                    "device\t{}\t{}\t{}",
                    camera.index(),
                    camera.human_name(),
                    camera.description()
                );
            }
        }
        Command::Grab { name, out, count } => {
            let device = configured_cameras
                .get(&name)
                .map_or(name.as_str(), String::as_str);
            let mut capture = CameraCapture::open(device)?;
            eprintln!("{name}: negotiated {}", capture.format());
            let started = Instant::now();
            let mut total_capture_wait = Duration::ZERO;
            let mut total_conversion = Duration::ZERO;
            for frame_index in 0..count {
                let frame = capture.capture()?;
                total_capture_wait += frame.capture_wait;
                total_conversion += frame.conversion_latency;
                if frame_index + 1 == count {
                    std::fs::write(&out, &frame.jpeg)?;
                    println!(
                        "captured_at_unix_ns={} dimensions={}x{} i420_bytes={}",
                        frame.captured_at_unix_ns,
                        frame.i420.width,
                        frame.i420.height,
                        frame.i420.data.len()
                    );
                }
            }
            let elapsed_seconds = started.elapsed().as_secs_f64();
            println!(
                "{name}: {count} frames, {:.2} fps; mean capture wait {:.2} ms; mean conversion latency {:.2} ms",
                f64::from(count) / elapsed_seconds,
                total_capture_wait.as_secs_f64() * 1000.0 / f64::from(count),
                total_conversion.as_secs_f64() * 1000.0 / f64::from(count)
            );
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cameras: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
