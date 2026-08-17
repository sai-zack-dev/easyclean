#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::Manager;

// ── State ────────────────────────────────────────────────────────────────────

struct ServerHandle(Mutex<ServerState>);

struct ServerState {
    child: Option<Child>,
    url: Option<String>,
}

// ── Config ───────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct AppConfig {
    media_root: String,
    port: u16,
    username: String,
    passcode: String,
    /// Persistent JWT signing secret. Generated once on first launch so tokens
    /// aren't forgeable with the server's hardcoded dev default. `#[serde(default)]`
    /// keeps older config.json files (without this key) loadable.
    #[serde(default)]
    secret: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            media_root: String::new(),
            port: 8080,
            username: "admin".to_string(),
            passcode: "easyclean".to_string(),
            secret: String::new(),
        }
    }
}

/// 32 random bytes as a hex string, for the JWT signing secret.
fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS RNG");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── Path helpers ─────────────────────────────────────────────────────────────

/// The workspace root (parent of the `desktop/` crate).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Path to the easyclean-server binary relative to the workspace root.
fn server_bin() -> PathBuf {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    workspace_root()
        .join("target")
        .join(profile)
        .join("easyclean-server")
}

/// Absolute path to the built web UI the spawned server should serve.
/// The server defaults to a *relative* `web/dist`, which only works when its
/// CWD happens to be the workspace root — so we pass an absolute path instead.
fn web_dist() -> PathBuf {
    workspace_root().join("web").join("dist")
}

fn config_file(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app data dir")
        .join("config.json")
}

// ── Phase 4.1 — LAN IP detection ─────────────────────────────────────────────

fn lan_ip() -> String {
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Generate a QR code SVG for `url` (white modules on transparent background).
fn qr_svg(url: &str) -> String {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = match QrCode::encode_text(url, QrCodeEcc::Medium) {
        Ok(q) => q,
        Err(_) => return String::new(),
    };
    let size = qr.size();
    let scale: i32 = 5;
    let border: i32 = 3;
    let dim = (size + border * 2) * scale;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{dim}" height="{dim}" viewBox="0 0 {dim} {dim}" shape-rendering="crispEdges">"#
    );
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                let px = (x + border) * scale;
                let py = (y + border) * scale;
                svg.push_str(&format!(
                    r##"<rect x="{px}" y="{py}" width="{scale}" height="{scale}" fill="#fff"/>"##
                ));
            }
        }
    }
    svg.push_str("</svg>");
    svg
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Phase 4.4 — Folder picker dialog.
#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    app.dialog().file().pick_folder(move |result| {
        let _ = tx.send(result);
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()
        .map(|p| p.to_string())
}

/// Phase 4.5 — Load persisted config (returns Default if none saved yet).
#[tauri::command]
fn load_config(app: tauri::AppHandle) -> AppConfig {
    std::fs::read_to_string(config_file(&app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist config to the app data dir (shared by `save_config` and `start_server`).
fn write_config(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_file(app);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Phase 4.5 — Persist config to app data dir.
#[tauri::command]
fn save_config(app: tauri::AppHandle, config: AppConfig) -> Result<(), String> {
    write_config(&app, &config)
}

/// Phase 4.3 — Start the easyclean-server as a child process.
/// Returns the LAN URL on success.
#[tauri::command]
fn start_server(
    handle: tauri::State<ServerHandle>,
    app: tauri::AppHandle,
    mut config: AppConfig,
) -> Result<String, String> {
    let mut state = handle.0.lock().unwrap();
    if state.child.is_some() {
        return state.url.clone().ok_or_else(|| "already running".into());
    }

    let bin = server_bin();
    if !bin.exists() {
        return Err(format!(
            "Server binary not found at {}.\nBuild it first:\n  cargo build -p easyclean-server",
            bin.display()
        ));
    }
    if config.media_root.is_empty() {
        return Err("No media folder selected.".into());
    }

    // The JWT secret is server-managed, not an editable UI field: reuse the
    // persisted one across restarts (so browser sessions survive Stop/Start),
    // generating it once on first launch. Without this the server falls back to
    // its hardcoded dev default, which is forgeable on the LAN.
    let persisted_secret = std::fs::read_to_string(config_file(&app))
        .ok()
        .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
        .map(|c| c.secret)
        .filter(|s| !s.is_empty());
    config.secret = persisted_secret.unwrap_or_else(random_secret);
    write_config(&app, &config)?;

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("server-data");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let mut child = Command::new(&bin)
        .env("EASYCLEAN_MEDIA_ROOT", &config.media_root)
        .env("EASYCLEAN_DATA_DIR", data_dir.to_string_lossy().as_ref())
        .env("EASYCLEAN_PORT", config.port.to_string())
        .env("EASYCLEAN_USER", &config.username)
        .env("EASYCLEAN_PASSCODE", &config.passcode)
        .env("EASYCLEAN_SECRET", &config.secret)
        .env("EASYCLEAN_WEB_DIST", web_dist().to_string_lossy().as_ref())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn server: {e}"))?;

    // Fail fast: if the child died immediately (e.g. port already in use), don't
    // leave the UI showing "Running" against a dead process.
    std::thread::sleep(std::time::Duration::from_millis(400));
    if let Ok(Some(status)) = child.try_wait() {
        return Err(format!(
            "Server exited on startup ({status}). Is port {} already in use?",
            config.port
        ));
    }

    let url = format!("http://{}:{}", lan_ip(), config.port);
    state.child = Some(child);
    state.url = Some(url.clone());
    Ok(url)
}

/// Phase 4.3 — Stop the running server.
#[tauri::command]
fn stop_server(handle: tauri::State<ServerHandle>) -> Result<(), String> {
    let mut state = handle.0.lock().unwrap();
    if let Some(mut child) = state.child.take() {
        child.kill().map_err(|e| e.to_string())?;
        child.wait().ok();
    }
    state.url = None;
    Ok(())
}

/// Returns `true` if the server child process is alive.
#[tauri::command]
fn server_status(handle: tauri::State<ServerHandle>) -> bool {
    let mut state = handle.0.lock().unwrap();
    if let Some(child) = state.child.as_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {
                state.child = None;
                state.url = None;
                false
            }
            Ok(None) => true,
            Err(_) => {
                state.child = None;
                state.url = None;
                false
            }
        }
    } else {
        false
    }
}

/// Returns the current server URL if running, else None.
#[tauri::command]
fn get_server_url(handle: tauri::State<ServerHandle>) -> Option<String> {
    handle.0.lock().unwrap().url.clone()
}

/// Phase 4.1 — Returns the primary LAN IPv4 address.
#[tauri::command]
fn get_lan_ip() -> String {
    lan_ip()
}

/// Phase 4.1 — Generate QR code SVG for the given URL.
#[tauri::command]
fn generate_qr(url: String) -> String {
    qr_svg(&url)
}

/// Phase 4.4 — Open URL in the system default browser.
#[tauri::command]
fn open_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(ServerHandle(Mutex::new(ServerState {
            child: None,
            url: None,
        })))
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            load_config,
            save_config,
            start_server,
            stop_server,
            server_status,
            get_server_url,
            get_lan_ip,
            generate_qr,
            open_browser,
        ])
        .on_window_event(|window, event| {
            // Kill the server when the main window is destroyed.
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let state = window.state::<ServerHandle>();
                let mut s = state.0.lock().unwrap();
                if let Some(mut child) = s.child.take() {
                    child.kill().ok();
                    child.wait().ok();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error running EasyClean desktop app");
}
