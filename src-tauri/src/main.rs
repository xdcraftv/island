#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicI32, AtomicU8, Ordering},
    mpsc::{channel, Receiver, Sender},
    Arc, Mutex,
};
use std::{thread, time::Duration};
use sysinfo::{Disks, Networks, System};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, PhysicalPosition,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Geolocation::Geolocator;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Mgr,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Media::Devices::{AudioDeviceRole, MediaDevice};
use windows::Storage::Streams::DataReader;
use windows::System::Power::{BatteryStatus, PowerManager};
use windows::Win32::Media::Audio::{
    eConsole, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

struct Ui(AtomicU8); // 0 idle, 1 peek, 2 open (reported by the page)
struct Cmd(Mutex<Sender<String>>);

#[derive(Serialize, Clone, Default)]
struct Media {
    none: bool,
    title: String,
    artist: String,
    playing: bool,
    pos: f64,
    dur: f64,
    cover: Option<String>,
}

#[tauri::command]
fn set_state(state: String, ui: tauri::State<'_, Ui>) {
    ui.0.store(
        match state.as_str() {
            "idle" => 0,
            "peek" => 1,
            _ => 2,
        },
        Ordering::Relaxed,
    );
}

#[tauri::command]
fn media_cmd(cmd: String, tx: tauri::State<'_, Cmd>) {
    let _ = tx.0.lock().unwrap().send(cmd);
}

#[tauri::command]
fn set_vol(delta: i32) -> Option<(i32, bool)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let v: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        let cur = v.GetMasterVolumeLevelScalar().ok()?;
        let next = (cur + (delta as f32 / 100.0)).clamp(0.0, 1.0);
        let _ = v.SetMasterVolumeLevelScalar(next, std::ptr::null());
        if delta > 0 {
            let _ = v.SetMute(false, std::ptr::null());
        }
        let muted = v.GetMute().ok()?.as_bool();
        Some(((next * 100.0).round() as i32, muted))
    }
}

fn thumb(p: &Props) -> Option<String> {
    let s = p.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
    let size = s.Size().ok()? as u32;
    if size == 0 {
        return None;
    }
    let r = DataReader::CreateDataReader(&s).ok()?;
    r.LoadAsync(size).ok()?.get().ok()?;
    let mut buf = vec![0u8; size as usize];
    r.ReadBytes(&mut buf).ok()?;
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(buf)))
}

fn read(s: &Session, last: &mut String) -> windows::core::Result<Media> {
    let p = s.TryGetMediaPropertiesAsync()?.get()?;
    let (title, artist) = (p.Title()?.to_string(), p.Artist()?.to_string());
    if title.is_empty() && artist.is_empty() {
        last.clear();
        return Ok(Media {
            none: true,
            ..Default::default()
        });
    }
    let t = s.GetTimelineProperties()?;
    let key = format!("{title}|{artist}");
    let cover = if *last != key {
        let c = thumb(&p);
        if c.is_some() {
            *last = key;
        }
        c
    } else {
        None
    };
    let playing = s.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
    let start = (t.StartTime()?.Duration as f64 / 1e7).max(0.0);
    let end = (t.EndTime()?.Duration as f64 / 1e7).max(start);
    let dur = end - start;
    let mut pos = (t.Position()?.Duration as f64 / 1e7 - start).max(0.0);
    if playing {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
            + 11644473600.0;
        let updated = t.LastUpdatedTime()?.UniversalTime as f64 / 1e7;
        if updated > 0.0 {
            pos += (now - updated).clamp(0.0, 5.0);
        }
    }
    Ok(Media {
        none: false,
        title,
        artist,
        playing,
        pos: if dur > 0.0 { pos.min(dur) } else { pos },
        dur,
        cover,
    })
}

fn media_loop(app: tauri::AppHandle, rx: Receiver<String>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut last = String::new();
    loop {
        let cmd = rx.recv_timeout(Duration::from_millis(1000)).ok();
        let session = Mgr::RequestAsync()
            .and_then(|o| o.get())
            .and_then(|m| m.GetCurrentSession());
        let Ok(s) = session else {
            last.clear();
            let _ = app.emit(
                "media",
                Media {
                    none: true,
                    ..Default::default()
                },
            );
            continue;
        };
        if let Some(c) = cmd {
            let _ = match c.as_str() {
                "toggle" => s.TryTogglePlayPauseAsync().and_then(|o| o.get()),
                "next" => s.TrySkipNextAsync().and_then(|o| o.get()),
                c if c.starts_with("seek:") => {
                    let start = s
                        .GetTimelineProperties()
                        .and_then(|t| t.StartTime())
                        .map(|st| st.Duration)
                        .unwrap_or(0);
                    let offset = (c[5..].parse::<f64>().unwrap_or(0.0) * 1e7) as i64;
                    s.TryChangePlaybackPositionAsync(start + offset)
                        .and_then(|o| o.get())
                }
                _ => s.TrySkipPreviousAsync().and_then(|o| o.get()),
            };
            thread::sleep(Duration::from_millis(80));
        }
        let m = read(&s, &mut last).unwrap_or_else(|_| {
            last.clear();
            Media {
                none: true,
                ..Default::default()
            }
        });
        let _ = app.emit("media", m);
    }
}

fn hover_loop(app: tauri::AppHandle) {
    let w = app.get_webview_window("main").unwrap();
    let mut last_inside = false;
    let mut last_ignore = true;
    loop {
        thread::sleep(Duration::from_millis(30));
        let (Ok(c), Ok(p), Ok(sz)) = (w.cursor_position(), w.outer_position(), w.inner_size())
        else {
            continue;
        };
        let sf = w.scale_factor().unwrap_or(1.0);
        let (x, y) = ((c.x - p.x as f64) / sf, (c.y - p.y as f64) / sf);
        let ui_state = app.state::<Ui>().0.load(Ordering::Relaxed);
        let (rw, rh) = match ui_state {
            0 => (176.0, 32.0),
            1 => (300.0, 48.0),
            _ => (520.0, 200.0),
        };
        let half = sz.width as f64 / sf / 2.0;
        let inside = x >= half - rw / 2.0 && x <= half + rw / 2.0 && y >= 0.0 && y <= rh;
        let should_ignore = !inside && ui_state != 2;
        if should_ignore != last_ignore {
            last_ignore = should_ignore;
            let _ = w.set_ignore_cursor_events(should_ignore);
        }
        if inside != last_inside {
            last_inside = inside;
            let _ = w.emit("hover", inside);
        }
    }
}

#[derive(Serialize, Clone)]
struct Sys {
    cpu: f32,
    ram_used: u64,
    ram_total: u64,
    gpu: f32,
    disk: f32,
    down: u64,
    up: u64,
    bat: i32,
    charging: bool,
}

#[tauri::command]
fn open_path(path: String) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

#[tauri::command]
async fn get_location() -> Result<(f64, f64), String> {
    tauri::async_runtime::spawn_blocking(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let e = |x: windows::core::Error| x.to_string();
        let g = Geolocator::new().map_err(e)?;
        let p = g.GetGeopositionAsync().map_err(e)?.get().map_err(e)?;
        let pos = p
            .Coordinate()
            .map_err(e)?
            .Point()
            .map_err(e)?
            .Position()
            .map_err(e)?;
        Ok((pos.Latitude, pos.Longitude))
    })
    .await
    .map_err(|x| x.to_string())?
}

fn gpu_now() -> Option<f32> {
    use std::os::windows::process::CommandExt;
    let o = std::process::Command::new("typeperf")
        .args(["\\GPU Engine(*)\\Utilization Percentage", "-sc", "1"])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    let mut it = t.lines().filter(|l| l.starts_with('"'));
    let (hd, vl) = (it.next()?, it.next()?);
    let cells = |l: &str| {
        l.trim()
            .trim_matches('"')
            .split("\",\"")
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let sum: f32 = cells(hd)
        .iter()
        .zip(cells(vl))
        .filter(|(h, _)| h.contains("engtype_3D"))
        .filter_map(|(_, v)| v.parse::<f32>().ok())
        .sum();
    Some(sum.min(100.0))
}

fn bat_now() -> (i32, bool) {
    let Ok(status) = PowerManager::BatteryStatus() else {
        return (-1, false);
    };
    if status == BatteryStatus::NotPresent {
        return (-1, false);
    }
    let pct = PowerManager::RemainingChargePercent().unwrap_or(-1);
    let charging = status == BatteryStatus::Charging;
    (pct, charging)
}

fn sys_loop(app: tauri::AppHandle) {
    let mut sys = System::new();
    let mut nets = Networks::new_with_refreshed_list();
    let gpu_shared = Arc::new(AtomicI32::new(-10));
    {
        let app_gpu = app.clone();
        let gpu_shared = Arc::clone(&gpu_shared);
        thread::spawn(move || loop {
            if app_gpu.state::<Ui>().0.load(Ordering::Relaxed) == 2 {
                let val = gpu_now().map(|v| (v * 10.0).round() as i32).unwrap_or(-10);
                gpu_shared.store(val, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(3000));
        });
    }
    let (mut n, mut disk) = (0u32, 0.0f32);
    loop {
        thread::sleep(Duration::from_millis(1000));
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        nets.refresh();
        if app.state::<Ui>().0.load(Ordering::Relaxed) != 2 {
            continue;
        }
        if n % 10 == 0 {
            let d = Disks::new_with_refreshed_list();
            let (t, a) = d.iter().fold((0u64, 0u64), |x, d| {
                (x.0 + d.total_space(), x.1 + d.available_space())
            });
            disk = if t > 0 {
                (t - a) as f32 * 100.0 / t as f32
            } else {
                0.0
            };
        }
        n = n.wrapping_add(1);
        let gpu = gpu_shared.load(Ordering::Relaxed) as f32 / 10.0;
        let (down, up) = nets.iter().fold((0, 0), |a, (_, d)| {
            (a.0 + d.received(), a.1 + d.transmitted())
        });
        let (bat, charging) = bat_now();
        let _ = app.emit(
            "sys",
            Sys {
                cpu: sys.global_cpu_usage(),
                ram_used: sys.used_memory(),
                ram_total: sys.total_memory(),
                gpu,
                disk,
                down,
                up,
                bat,
                charging,
            },
        );
    }
}

fn clip_loop(app: tauri::AppHandle) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let Ok(mut cb) = arboard::Clipboard::new() else {
        return;
    };
    let (mut last_t, mut last_i) = (String::new(), 0u64);
    loop {
        thread::sleep(Duration::from_millis(700));
        if let Ok(t) = cb.get_text() {
            if !t.is_empty() && t != last_t {
                last_t = t.clone();
                let _ = app.emit(
                    "clip",
                    serde_json::json!({ "text": t.chars().take(500).collect::<String>() }),
                );
                continue;
            }
        }
        if let Ok(img) = cb.get_image() {
            if img.width == 0
                || img.height == 0
                || img.bytes.len() < img.width * img.height * 4
            {
                continue;
            }
            let h = img
                .bytes
                .iter()
                .step_by(997)
                .fold((img.width as u64) << 20 ^ img.height as u64, |a, b| {
                    a.wrapping_mul(31).wrapping_add(*b as u64)
                });
            if h != last_i {
                last_i = h;
                let (sw, sh) = (
                    160.min(img.width),
                    (img.height * 160.min(img.width) / img.width).max(1),
                );
                let mut out = Vec::with_capacity(sw * sh * 4);
                for y in 0..sh {
                    for x in 0..sw {
                        let i = ((y * img.height / sh) * img.width + x * img.width / sw) * 4;
                        out.extend_from_slice(&img.bytes[i..i + 4]);
                    }
                }
                let _ = app.emit(
                    "clip",
                    serde_json::json!({ "w": sw, "h": sh, "rgba": STANDARD.encode(out) }),
                );
            }
        }
    }
}

fn vol_now() -> Option<(f32, bool)> {
    unsafe {
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let v: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        Some((
            v.GetMasterVolumeLevelScalar().ok()?,
            v.GetMute().ok()?.as_bool(),
        ))
    }
}

fn audio_loop(app: tauri::AppHandle) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let (mut lv, mut lm, mut lid, mut n) = (-1i32, false, String::new(), 0u32);
    loop {
        thread::sleep(Duration::from_millis(250));
        n = n.wrapping_add(1);
        if let Some((v, m)) = vol_now() {
            let p = (v * 100.0).round() as i32;
            if lv >= 0 && (p != lv || m != lm) {
                let _ = app.emit("vol", serde_json::json!({ "pct": p, "muted": m }));
            }
            lv = p;
            lm = m;
        }
        if n % 4 == 0 {
            if let Ok(id) = MediaDevice::GetDefaultAudioRenderId(AudioDeviceRole::Default) {
                let id = id.to_string();
                if id != lid {
                    if !lid.is_empty() {
                        let name = DeviceInformation::CreateFromIdAsync(
                            &windows::core::HSTRING::from(id.as_str()),
                        )
                        .and_then(|o| o.get())
                        .and_then(|d| d.Name())
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                        let _ = app.emit("audiodev", name);
                    }
                    lid = id;
                }
            }
        }
    }
}

fn main() {
    let (tx, rx) = channel::<String>();
    tauri::Builder::default()
        .manage(Ui(AtomicU8::new(0)))
        .manage(Cmd(Mutex::new(tx)))
        .invoke_handler(tauri::generate_handler![
            set_state,
            media_cmd,
            set_vol,
            open_path,
            get_location
        ])
        .setup(move |app| {
            let w = app.get_webview_window("main").unwrap();
            if let Ok(Some(m)) = w.current_monitor() {
                let x = m.position().x + (m.size().width as i32 - w.outer_size()?.width as i32) / 2;
                w.set_position(PhysicalPosition::new(x, m.position().y))?;
            }
            let show = MenuItem::with_id(app, "show", "Open Island", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Island", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Island")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            app.state::<Ui>().0.store(2, Ordering::Relaxed);
                            let _ = window.show();
                            let _ = window.set_ignore_cursor_events(false);
                            let _ = window.set_focus();
                            let _ = window.emit("open-island", ());
                        }
                    }
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            w.set_ignore_cursor_events(true)?;
            let h = app.handle().clone();
            thread::spawn(move || hover_loop(h));
            let h = app.handle().clone();
            thread::spawn(move || media_loop(h, rx));
            let h = app.handle().clone();
            thread::spawn(move || sys_loop(h));
            let h = app.handle().clone();
            thread::spawn(move || clip_loop(h));
            let h = app.handle().clone();
            thread::spawn(move || audio_loop(h));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Island");
}
