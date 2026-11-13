use clap::Parser;
use livekit_camera_publisher::{capture, config::CameraConfiguration, publisher};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(about = "Publish a local camera to a LiveKit room")]
struct Arguments {
    /// `LiveKit` server URL.
    #[arg(long, env = "LIVEKIT_URL")]
    url: String,
    /// A token granting camera publication to the desired room.
    #[arg(long, env = "LIVEKIT_TOKEN", hide_env_values = true)]
    token: String,
    /// Track name: 1–40 lowercase letters, digits or underscores.
    #[arg(long)]
    name: String,
    /// Camera index, device path or native identifier.
    #[arg(long)]
    device: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "__capture")
    {
        capture::run_capture_process()?;
        return Ok(());
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse();
    let camera = CameraConfiguration::new(arguments.name, arguments.device)?;
    cameras::initialize()?;
    let stopped = CancellationToken::new();
    let signal_stopped = stopped.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            if let Ok(mut termination) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = termination.recv() => {} }
            } else {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
        signal_stopped.cancel();
    });
    publisher::publish_camera(camera, &arguments.url, &arguments.token, stopped).await?;
    Ok(())
}
