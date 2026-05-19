mod embedded;
mod ui_assets;
mod ui_updater;

use anyhow::{Context, Result};
use clap::Parser;
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use tao::window::Icon as WindowIcon;
use tao::window::{Window, WindowBuilder};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    Icon as TrayImage, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use wry::{WebView, WebViewBuilder};

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/vibe-plus-logo.png");
const DEFAULT_MIN_WIDTH: u32 = 720;
const DEFAULT_MIN_HEIGHT: u32 = 480;
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 820;
const FLOATING_WIDTH: u32 = 720;
const FLOATING_HEIGHT: u32 = 520;
const FLOATING_MIN_WIDTH: u32 = 360;
const FLOATING_MIN_HEIGHT: u32 = 260;
const DEFAULT_GATEWAY_PORT: u16 = 15917;
const SHELL_POLL_INTERVAL: Duration = Duration::from_secs(30);
const UPDATE_READY_MARKER: &str = "update-downloaded-ready";

/// Dev-mode URL: skip embedded gateway, load from Vite HMR server.
const DEV_FRONTEND_URL: &str = "http://127.0.0.1:15876";

/// Production URL: embedded Vue assets served via the `app://` custom protocol.
const PROD_FRONTEND_URL: &str = "app://localhost/ui";

const LOADING_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Vibe Plus</title>
<style>
  *{box-sizing:border-box;margin:0;padding:0}
  body{background:#0d0d0d;display:flex;align-items:center;justify-content:center;height:100vh;font-family:system-ui,-apple-system,sans-serif}
  .wrap{text-align:center}
  .ring{width:44px;height:44px;border:3px solid #2a2a2a;border-top-color:#8b5cf6;border-radius:50%;animation:spin .7s linear infinite;margin:0 auto}
  @keyframes spin{to{transform:rotate(360deg)}}
  p{color:#555;margin-top:18px;font-size:13px;letter-spacing:.02em}
  .logo{font-size:20px;font-weight:700;color:#8b5cf6;margin-bottom:32px;letter-spacing:-.02em}
</style>
</head>
<body>
<div class="wrap">
  <div class="logo">Vibe Plus</div>
  <div class="ring"></div>
  <p>Bringing up gateway…</p>
</div>
</body>
</html>"#;

#[derive(Debug, Parser)]
#[command(
    name = "vibe-app",
    version,
    about = "Vibe Plus all-in-one desktop app."
)]
struct Args {
    /// Skip embedded gateway; connect to an already-running server (for development).
    #[arg(long)]
    dev: bool,

    /// Override the URL loaded in the window (implies --dev).
    #[arg(long)]
    url: Option<String>,

    /// Window width in logical pixels.
    #[arg(long)]
    width: Option<u32>,

    /// Window height in logical pixels.
    #[arg(long)]
    height: Option<u32>,

    /// Minimum window width in logical pixels.
    #[arg(long)]
    min_width: Option<u32>,

    /// Minimum window height in logical pixels.
    #[arg(long)]
    min_height: Option<u32>,

    /// Open as a compact floating utility window.
    #[arg(long)]
    floating: bool,

    /// Keep the window above other windows.
    #[arg(long)]
    always_on_top: bool,

    /// Hide native window decorations.
    #[arg(long)]
    frameless: bool,

    /// Request a transparent window/webview background.
    #[arg(long)]
    transparent: bool,

    /// Hide the floating window when it loses focus.
    #[arg(long)]
    hide_on_blur: bool,

    /// Keep the floating window visible when it loses focus.
    #[arg(long)]
    no_hide_on_blur: bool,

    /// Disable the system tray icon.
    #[arg(long)]
    no_tray: bool,

    /// Gateway port for the embedded server and status checks.
    #[arg(long, default_value_t = DEFAULT_GATEWAY_PORT)]
    gateway_port: u16,
}

#[derive(Debug, Clone)]
enum UserEvent {
    Menu(MenuEvent),
    Tray(TrayIconEvent),
    /// Emitted by the background thread once the gateway is responding.
    GatewayReady,
}

#[derive(Debug, Clone)]
struct TrayMenuIds {
    show: MenuId,
    hide: MenuId,
    gateway_status: MenuId,
    takeover: MenuId,
    restart_update: MenuId,
    quit: MenuId,
}

struct TrayParts {
    _icon: TrayIcon,
    menu: Menu,
    gateway_separator: PredefinedMenuItem,
    gateway_status: MenuItem,
    takeover: MenuItem,
    gateway_visible: bool,
    restart_update: MenuItem,
    restart_update_visible: bool,
    menu_ids: TrayMenuIds,
}

#[derive(Debug, Clone)]
struct ShellStatus {
    gateway_online: bool,
    gateway_label: String,
    update_ready: bool,
}

fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    run(args)
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vibe_app=info,vibe_core=info".into()),
        )
        .init();
}

fn run(args: Args) -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Tray(event));
    }));
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));

    let floating = args.floating;
    let always_on_top = args.always_on_top;
    let frameless = args.frameless;
    let transparent = args.transparent;
    let hide_on_blur = args.hide_on_blur || (floating && !args.no_hide_on_blur && !args.no_tray);
    let default_width = if floating {
        FLOATING_WIDTH
    } else {
        DEFAULT_WIDTH
    };
    let default_height = if floating {
        FLOATING_HEIGHT
    } else {
        DEFAULT_HEIGHT
    };
    let default_min_width = if floating {
        FLOATING_MIN_WIDTH
    } else {
        DEFAULT_MIN_WIDTH
    };
    let default_min_height = if floating {
        FLOATING_MIN_HEIGHT
    } else {
        DEFAULT_MIN_HEIGHT
    };
    let width = args.width.unwrap_or(default_width);
    let height = args.height.unwrap_or(default_height);
    let min_width = args.min_width.unwrap_or(default_min_width);
    let min_height = args.min_height.unwrap_or(default_min_height);
    let close_hides = !args.no_tray;
    let gateway_port = args.gateway_port;

    // In dev/url-override mode we skip the embedded gateway and just load the URL directly.
    let dev_mode = args.dev || args.url.is_some();
    let ui_url = if let Some(u) = args.url {
        u
    } else if dev_mode {
        DEV_FRONTEND_URL.to_owned()
    } else {
        PROD_FRONTEND_URL.to_owned()
    };

    let window = WindowBuilder::new()
        .with_title("Vibe Plus")
        .with_inner_size(LogicalSize::new(width, height))
        .with_min_inner_size(LogicalSize::new(min_width, min_height))
        .with_always_on_top(always_on_top)
        .with_decorations(!frameless)
        .with_transparent(transparent)
        .with_window_icon(Some(build_window_icon()?))
        .build(&event_loop)
        .context("failed to create Vibe Plus window")?;

    let mut tray = if args.no_tray {
        None
    } else {
        Some(build_tray().context("failed to create tray icon")?)
    };

    // In embedded mode we show the loading page first; navigate to ui_url on GatewayReady.
    let initial_builder = WebViewBuilder::new()
        .with_initialization_script(
            "window.__vibeOpenUrls=[];window.__vibeReceiveOpenUrl=(url)=>{window.__vibeOpenUrls.push(url);window.dispatchEvent(new CustomEvent('vibe:native-open-url',{detail:url}));};",
        )
        .with_transparent(transparent);

    let initial_builder = if dev_mode {
        initial_builder.with_url(&ui_url)
    } else {
        // Register the custom `app://` protocol to serve embedded Vue assets,
        // then show the loading screen while the embedded gateway starts up.
        initial_builder
            .with_custom_protocol("app".to_string(), ui_assets::handle)
            .with_html(LOADING_HTML)
    };

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let webview = initial_builder
        .build(&window)
        .context("failed to create Vibe Plus webview")?;

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .context("failed to get GTK container")?;
        initial_builder
            .build_gtk(vbox)
            .context("failed to create Vibe Plus webview")?
    };

    // Spawn the embedded gateway unless we're in dev mode.
    if !dev_mode {
        let proxy = event_loop.create_proxy();
        let embedded_ui_version = ui_assets::embedded_ui_version();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime for embedded gateway");
            rt.block_on(async move {
                tokio::spawn(async move {
                    if let Err(e) = embedded::start(gateway_port).await {
                        tracing::error!("embedded gateway exited: {e:#}");
                    }
                });
                embedded::wait_until_ready(gateway_port).await;
                proxy.send_event(UserEvent::GatewayReady).ok();

                // Background: check GitHub Pages for a newer UI build and
                // download it to ~/.vibe/ui-cache/ if one is available.
                // Runs after the gateway is up so startup latency is unaffected.
                tokio::spawn(ui_updater::check_and_update(embedded_ui_version));
            });
        });
    }

    let mut ignore_next_blur = false;
    let mut last_shell_poll = Instant::now() - SHELL_POLL_INTERVAL;

    event_loop.run(move |event, event_loop, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                let _keep_tray_alive = &tray;
                if let Some(tray) = tray.as_mut() {
                    update_tray_status(tray, &check_shell_status(gateway_port));
                }
            }
            Event::NewEvents(_) => {
                if last_shell_poll.elapsed() >= SHELL_POLL_INTERVAL {
                    last_shell_poll = Instant::now();
                    if let Some(tray) = tray.as_mut() {
                        update_tray_status(tray, &check_shell_status(gateway_port));
                    }
                }
            }
            Event::UserEvent(UserEvent::GatewayReady) => {
                let _ = webview.load_url(&ui_url);
                if let Some(tray) = tray.as_mut() {
                    update_tray_status(tray, &check_shell_status(gateway_port));
                }
            }
            Event::Opened { urls } => {
                show_window(&window);
                ignore_next_blur = true;
                for url in urls {
                    dispatch_open_url(&webview, url.as_str());
                }
            }
            Event::Reopen { .. } => {
                show_window(&window);
                ignore_next_blur = true;
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                if tray
                    .as_ref()
                    .is_some_and(|tray| event.id == tray.menu_ids.show)
                {
                    show_window(&window);
                    ignore_next_blur = true;
                } else if tray
                    .as_ref()
                    .is_some_and(|tray| event.id == tray.menu_ids.hide)
                {
                    hide_window(&window, event_loop);
                } else if tray
                    .as_ref()
                    .is_some_and(|tray| event.id == tray.menu_ids.gateway_status)
                {
                    if let Some(tray) = tray.as_mut() {
                        update_tray_status(tray, &check_shell_status(gateway_port));
                    }
                } else if tray
                    .as_ref()
                    .is_some_and(|tray| event.id == tray.menu_ids.takeover)
                {
                    show_window(&window);
                    ignore_next_blur = true;
                } else if tray
                    .as_ref()
                    .is_some_and(|tray| event.id == tray.menu_ids.restart_update)
                {
                    clear_update_ready_marker();
                    restart_to_update(control_flow);
                } else if tray
                    .as_ref()
                    .is_some_and(|tray| event.id == tray.menu_ids.quit)
                {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::Click {
                button_state: MouseButtonState::Up,
                ..
            }))
            | Event::UserEvent(UserEvent::Tray(TrayIconEvent::DoubleClick { .. })) => {
                if toggle_window(&window, event_loop) {
                    ignore_next_blur = true;
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } if close_hides => {
                hide_window(&window, event_loop);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } if hide_on_blur && ignore_next_blur => {
                ignore_next_blur = false;
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } if hide_on_blur => {
                hide_window(&window, event_loop);
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(true),
                ..
            } => {
                ignore_next_blur = false;
            }
            _ => {}
        }
    });
}

fn show_window(window: &Window) {
    window.set_visible(true);
    window.set_minimized(false);
    window.set_focus();
}

fn toggle_window<T>(window: &Window, event_loop: &EventLoopWindowTarget<T>) -> bool {
    if window.is_visible() {
        hide_window(window, event_loop);
        false
    } else {
        show_window(window);
        true
    }
}

fn hide_window<T>(window: &Window, event_loop: &EventLoopWindowTarget<T>) {
    window.set_visible(false);
    hide_application(event_loop);
}

fn dispatch_open_url(webview: &WebView, url: &str) {
    let Ok(encoded) = serde_json::to_string(url) else {
        return;
    };
    let script = format!("window.__vibeReceiveOpenUrl?.({encoded});");
    let _ = webview.evaluate_script(&script);
}

fn build_tray() -> Result<TrayParts> {
    let show = MenuItem::new("Show Vibe Plus", true, None);
    let hide = MenuItem::new("Hide", true, None);
    let gateway_separator = PredefinedMenuItem::separator();
    let gateway_status = MenuItem::new("Gateway ready", false, None);
    let takeover = MenuItem::new("Open gateway controls", true, None);
    let restart_update = MenuItem::new("Restart to update", false, None);
    let quit = MenuItem::new("Quit Vibe Plus", true, "cmd+q".parse().ok());
    let menu = Menu::with_items(&[&show, &hide, &PredefinedMenuItem::separator(), &quit])
        .context("failed to build tray menu")?;
    let menu_ids = TrayMenuIds {
        show: show.id().clone(),
        hide: hide.id().clone(),
        gateway_status: gateway_status.id().clone(),
        takeover: takeover.id().clone(),
        restart_update: restart_update.id().clone(),
        quit: quit.id().clone(),
    };
    let icon = build_tray_image()?;
    let tray_icon = TrayIconBuilder::new()
        .with_tooltip("Vibe Plus")
        .with_title("Vibe+")
        .with_menu(Box::new(menu.clone()))
        .with_icon_as_template(false)
        .with_icon(icon)
        .with_menu_on_left_click(false)
        .build()
        .context("failed to build tray icon")?;

    Ok(TrayParts {
        _icon: tray_icon,
        menu,
        gateway_separator,
        gateway_status,
        takeover,
        gateway_visible: false,
        restart_update,
        restart_update_visible: false,
        menu_ids,
    })
}

fn update_tray_status(tray: &mut TrayParts, status: &ShellStatus) {
    tray.gateway_status.set_text(&status.gateway_label);
    tray.gateway_status.set_enabled(false);
    tray.takeover.set_enabled(status.gateway_online);
    tray.takeover.set_text("Gateway and takeover");
    if status.gateway_online && !tray.gateway_visible {
        let insert_at = if tray.restart_update_visible {
            tray.menu.items().len().saturating_sub(3)
        } else {
            tray.menu.items().len().saturating_sub(2)
        };
        let _ = tray.menu.insert(&tray.gateway_separator, insert_at);
        let _ = tray.menu.insert(&tray.gateway_status, insert_at + 1);
        let _ = tray.menu.insert(&tray.takeover, insert_at + 2);
        tray.gateway_visible = true;
    } else if !status.gateway_online && tray.gateway_visible {
        let _ = tray.menu.remove(&tray.takeover);
        let _ = tray.menu.remove(&tray.gateway_status);
        let _ = tray.menu.remove(&tray.gateway_separator);
        tray.gateway_visible = false;
    }
    if status.update_ready && !tray.restart_update_visible {
        let insert_at = tray.menu.items().len().saturating_sub(2);
        let _ = tray.menu.insert(&tray.restart_update, insert_at);
        tray.restart_update_visible = true;
    } else if !status.update_ready && tray.restart_update_visible {
        let _ = tray.menu.remove(&tray.restart_update);
        tray.restart_update_visible = false;
    }
}

fn check_shell_status(port: u16) -> ShellStatus {
    let gateway_online = gateway_responds(port);
    ShellStatus {
        gateway_online,
        gateway_label: if gateway_online {
            format!("Gateway on :{port}")
        } else {
            "Gateway unavailable".into()
        },
        update_ready: update_ready_marker_path().is_some_and(|p| p.exists()),
    }
}

fn gateway_responds(port: u16) -> bool {
    let addr: SocketAddr = match format!("127.0.0.1:{port}").parse() {
        Ok(addr) => addr,
        Err(_) => return false,
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(180)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(220)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(220)));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = [0_u8; 64];
    stream
        .read(&mut buf)
        .is_ok_and(|n| std::str::from_utf8(&buf[..n]).is_ok_and(|s| s.contains(" 200 ")))
}

fn vibe_home() -> Option<PathBuf> {
    let home = std::env::var_os("VIBE_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::UserDirs::new().map(|d| d.home_dir().join(".vibe")))?;
    Some(home)
}

fn update_ready_marker_path() -> Option<PathBuf> {
    vibe_home().map(|home| home.join(UPDATE_READY_MARKER))
}

fn clear_update_ready_marker() {
    if let Some(path) = update_ready_marker_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn restart_to_update(control_flow: &mut ControlFlow) {
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        cmd.args(std::env::args_os().skip(1));
        let _ = cmd.spawn();
    }
    *control_flow = ControlFlow::Exit;
}

#[cfg(target_os = "macos")]
fn hide_application<T>(event_loop: &EventLoopWindowTarget<T>) {
    use tao::platform::macos::EventLoopWindowTargetExtMacOS;
    event_loop.hide_application();
}

#[cfg(not(target_os = "macos"))]
fn hide_application<T>(_event_loop: &EventLoopWindowTarget<T>) {}

fn build_window_icon() -> Result<WindowIcon> {
    let (rgba, width, height) = decode_app_icon_png()?;
    WindowIcon::from_rgba(rgba, width, height).context("invalid window icon")
}

fn build_tray_image() -> Result<TrayImage> {
    let (rgba, width, height) = decode_app_icon_png()?;
    TrayImage::from_rgba(rgba, width, height).context("invalid tray icon")
}

fn decode_app_icon_png() -> Result<(Vec<u8>, u32, u32)> {
    let decoder = png::Decoder::new(Cursor::new(APP_ICON_PNG));
    let mut reader = decoder.read_info().context("failed to decode app icon")?;
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .context("invalid app icon buffer")?
    ];
    let info = reader
        .next_frame(&mut buf)
        .context("failed to read app icon frame")?;
    let bytes = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .chunks_exact(3)
            .flat_map(|px| [px[0], px[1], px[2], 255])
            .collect(),
        color_type => anyhow::bail!("unsupported app icon color type: {color_type:?}"),
    };
    Ok((rgba, info.width, info.height))
}
