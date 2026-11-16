//! A capture process owns a camera and runs in its own process group, so it must exit by
//! itself when the supervisor that spawned it dies without cleaning up.

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn process_is_running(process_id: &str) -> bool {
    Command::new("kill")
        .args(["-0", process_id])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn a_capture_process_exits_after_its_supervisor_dies() -> Result<(), Box<dyn std::error::Error>> {
    // The shell is the supervisor: it starts the capture process on a device that cannot
    // open, reports its id and exits (passing its own id as the supervisor), leaving the capture process orphaned.
    let launcher = format!(
        "'{}' __capture orphan /nonexistent/camera $$ >/dev/null 2>&1 </dev/null & echo $!; sleep 1",
        env!("CARGO_BIN_EXE_livekit-camera-publisher")
    );
    let output = Command::new("sh").args(["-c", &launcher]).output()?;
    let process_id = String::from_utf8(output.stdout)?.trim().to_owned();
    assert!(!process_id.is_empty(), "the launcher reported a process id");

    let deadline = Instant::now() + Duration::from_secs(20);
    while process_is_running(&process_id) {
        assert!(
            Instant::now() < deadline,
            "capture process {process_id} outlived its supervisor"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}
