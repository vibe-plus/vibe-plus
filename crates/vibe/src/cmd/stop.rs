use anyhow::Result;
use vibe_core::paths;

pub fn run() -> Result<()> {
    let pid_path = paths::pid_path()?;
    if !pid_path.exists() {
        println!("vibe is not running (no pid file).");
        return Ok(());
    }
    let pid_s = std::fs::read_to_string(&pid_path)?;
    let pid: u32 = pid_s.trim().parse()?;
    #[cfg(unix)]
    {
        unsafe {
            if libc::kill(pid as i32, libc::SIGTERM) == 0 {
                let _ = std::fs::remove_file(&pid_path);
                println!("vibe stopped (pid {pid}).");
            } else {
                println!("Failed to stop pid {pid} — maybe already stopped.");
                let _ = std::fs::remove_file(&pid_path);
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        let _ = std::fs::remove_file(&pid_path);
        match status {
            Ok(s) if s.success() => println!("vibe stopped (pid {pid})."),
            _ => println!("vibe stopped (cleared pid file; process {pid} may already be gone)."),
        }
    }
    Ok(())
}
