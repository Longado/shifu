//! Official payment handoffs leave the unprivileged meal webview for the system browser.
use std::sync::Mutex;

#[derive(Default)]
pub struct MainRestore(Mutex<Option<MainFrame>>);

struct MainFrame {
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    maximized: bool,
}

pub fn allowed(url: &tauri::Url) -> bool {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
        || url.port().is_some_and(|port| port != 443) { return false; }
    match url.host_str() {
        Some("r.ele.me") => url.path() == "/payment",
        Some("h5.ele.me") => url.path().starts_with("/cashier/postcashier"),
        Some("cashier.ele.me") => true,
        Some("tb.ele.me") => url.path() == "/app/eleme-newretail-c-h5/order-detail-unicorn/order-detail",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_known_https_cashier_destinations() {
        for value in ["https://r.ele.me/payment?orderId=1", "https://h5.ele.me/cashier/postcashier/", "https://cashier.ele.me/", "https://tb.ele.me/app/eleme-newretail-c-h5/order-detail-unicorn/order-detail?orderId=1"] {
            assert!(allowed(&value.parse().unwrap()), "{value}");
        }
        for value in ["http://r.ele.me/payment", "https://r.ele.me.evil.example/payment", "https://evil.example/", "https://r.ele.me/other", "https://user@r.ele.me/payment", "https://r.ele.me:444/payment", "file:///etc/passwd"] {
            assert!(!allowed(&value.parse().unwrap()), "{value}");
        }
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    #[test]
    fn uses_free_space_on_the_right_without_moving_main() {
        let (main, side) = beside(Rect { x: 100., y: 40., width: 800., height: 700. }, Rect { x: 0., y: 25., width: 1920., height: 1000. });
        assert!(main.is_none()); assert_eq!(side.x, 912.); assert_eq!(side.width, 480.);
    }
    #[test]
    fn tiles_on_a_laptop_and_respects_external_monitor_origin() {
        let area = Rect { x: -1512., y: 25., width: 1512., height: 900. };
        let (main, side) = beside(Rect { x: -1400., y: 50., width: 1200., height: 800. }, area);
        let main = main.unwrap();
        assert!(main.width >= 720.); assert_eq!(main.x + main.width + 12., side.x);
        assert_eq!(side.x + side.width, 0.); assert!(side.y + side.height <= 925.);
    }
}

#[derive(Clone, Copy)]
struct Rect { x: f64, y: f64, width: f64, height: f64 }

fn beside(main: Rect, area: Rect) -> (Option<Rect>, Rect) {
    let width = 480.0_f64.min(area.width);
    let height = main.height.min(area.height);
    let y = main.y.clamp(area.y, area.y + area.height - height);
    let right = main.x + main.width + 12.;
    if right + width <= area.x + area.width {
        return (None, Rect { x: right, y, width, height });
    }
    let x = area.x + area.width - width;
    // On normal laptop displays, keep both windows visible. Very narrow displays
    // retain the main window's minimum width and pin the meal window to the right.
    let resized = (area.width >= 1212.).then_some(Rect { x: area.x, y,
        width: area.width - width - 12., height });
    (resized, Rect { x, y, width, height })
}

pub fn place(app: &tauri::AppHandle, meal: &tauri::WebviewWindow) -> Result<(), String> {
    use tauri::Manager;
    let main = app.get_webview_window("main").ok_or("找不到 Shifu 主窗口")?;
    let monitor = main.current_monitor().map_err(|e| e.to_string())?.ok_or("找不到当前屏幕")?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let pos = main.outer_position().map_err(|e| e.to_string())?;
    let size = main.outer_size().map_err(|e| e.to_string())?;
    let (adjusted, side) = beside(Rect { x: pos.x as f64 / scale, y: pos.y as f64 / scale,
        width: size.width as f64 / scale, height: size.height as f64 / scale },
        Rect { x: area.position.x as f64 / scale, y: area.position.y as f64 / scale,
            width: area.size.width as f64 / scale, height: area.size.height as f64 / scale });
    if let Some(rect) = adjusted {
        // Keep the frame from the first displacement, even if the meal entry is clicked again.
        app.state::<MainRestore>().0.lock().map_err(|e| e.to_string())?.get_or_insert(MainFrame {
            position: pos,
            size: main.inner_size().map_err(|e| e.to_string())?,
            maximized: main.is_maximized().map_err(|e| e.to_string())?,
        });
        main.unmaximize().map_err(|e| e.to_string())?;
        main.set_size(tauri::LogicalSize::new(rect.width, rect.height)).map_err(|e| e.to_string())?;
        main.set_position(tauri::LogicalPosition::new(rect.x, rect.y)).map_err(|e| e.to_string())?;
    }
    meal.set_size(tauri::LogicalSize::new(side.width, (side.height - 28.).max(480.))).map_err(|e| e.to_string())?;
    meal.set_position(tauri::LogicalPosition::new(side.x, side.y)).map_err(|e| e.to_string())
}

pub fn restore(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let saved = app.state::<MainRestore>().0.lock().map_err(|e| e.to_string())?.take();
    let (Some(frame), Some(main)) = (saved, app.get_webview_window("main")) else { return Ok(()); };
    main.set_size(frame.size).map_err(|e| e.to_string())?;
    main.set_position(frame.position).map_err(|e| e.to_string())?;
    if frame.maximized { main.maximize().map_err(|e| e.to_string())?; }
    Ok(())
}
