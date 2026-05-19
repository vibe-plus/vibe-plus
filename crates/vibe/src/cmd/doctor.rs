use anyhow::Result;
use vibe_core::paths;
use vibe_i18n::text_env;

pub async fn run() -> Result<()> {
    println!("=== {} ===\n", text_env("cli-doctor-title"));

    // 1. pid / process
    let pid_path = paths::pid_path()?;
    let running = if pid_path.exists() {
        let pid_s = std::fs::read_to_string(&pid_path).unwrap_or_default();
        let pid: u32 = pid_s.trim().parse().unwrap_or(0);
        #[cfg(unix)]
        let alive = pid > 0 && unsafe { libc::kill(pid as i32, 0) == 0 };
        #[cfg(not(unix))]
        let alive = false;
        if alive {
            print!("[ok]  ");
            println!("process running (pid {pid})");
            true
        } else {
            print!("[!!]  ");
            println!("pid file exists but process is dead");
            false
        }
    } else {
        print!("[--]  ");
        println!("not running");
        false
    };

    // 2. port reachable
    if running {
        let base_url = super::configured_base_url()?;
        let url = format!("{base_url}/health");
        match reqwest::get(&url).await {
            Ok(r) if r.status().is_success() => {
                print!("[ok]  ");
                println!("gateway reachable at {base_url}");
            }
            _ => {
                print!("[!!]  ");
                println!("gateway not responding at {base_url}");
            }
        }
    }

    // 3. DB
    let db_path = paths::db_path()?;
    if db_path.exists() {
        print!("[ok]  ");
        println!("db at {}", db_path.display());
    } else {
        print!("[--]  ");
        println!("db not created yet (run vibe first)");
    }

    println!();
    if running {
        println!("Providers: open the dashboard with `vibe ui` to view/edit configured providers.");
    } else {
        println!("Run `vibe up` to bring up the local proxy.");
    }
    Ok(())
}
