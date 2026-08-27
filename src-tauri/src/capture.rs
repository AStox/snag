use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::{Result, SnagError};
use crate::image_util;
use crate::models::{fixtures, CaptureBundle, FixtureMeta};

static DEMO_INDEX: AtomicUsize = AtomicUsize::new(0);

pub fn pick_fixture(pref: &str) -> FixtureMeta {
    let all = fixtures();
    if pref != "auto" {
        if let Some(f) = all.iter().find(|f| f.id == pref) {
            return f.clone();
        }
    }
    let i = DEMO_INDEX.fetch_add(1, Ordering::Relaxed);
    all[i % all.len()].clone()
}

pub fn capture_demo(pref: &str) -> Result<CaptureBundle> {
    let f = pick_fixture(pref);
    let (full, crop) = image_util::mark_and_pack(f.png, f.cursor_x, f.cursor_y)?;
    Ok(CaptureBundle {
        full_png: full,
        crop_png: crop,
        cursor_x: f.cursor_x as f64,
        cursor_y: f.cursor_y as f64,
        source_app: Some(f.source_app.into()),
        window_title: Some(f.window_title.into()),
        fixture_id: Some(f.id.into()),
        document_text: None,
    })
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use core_foundation::base::{CFGetTypeID, CFRelease, TCFType};
    use core_foundation::data::CFData;
    use core_foundation::string::CFString;
    use core_graphics::display::{CGDisplay, CGPoint};
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use core_graphics::geometry::CGRect;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetDisplaysWithPoint(
            point: CGPoint,
            max_displays: u32,
            displays: *mut u32,
            matching_display_count: *mut u32,
        ) -> i32;
        fn CGDisplayBounds(display: u32) -> CGRect;
        fn CGDisplayCreateImage(display: u32) -> *mut std::ffi::c_void;
        fn CGDisplayPixelsWide(display: u32) -> usize;
        fn CGDisplayPixelsHigh(display: u32) -> usize;
        fn CGImageGetWidth(image: *mut std::ffi::c_void) -> usize;
        fn CGImageGetHeight(image: *mut std::ffi::c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *mut std::ffi::c_void) -> usize;
        fn CGImageGetBitsPerPixel(image: *mut std::ffi::c_void) -> usize;
        fn CGImageGetDataProvider(image: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn CGDataProviderCopyData(provider: *mut std::ffi::c_void) -> *const std::ffi::c_void;
        fn CGImageRelease(image: *mut std::ffi::c_void);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> *mut std::ffi::c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut std::ffi::c_void,
            attribute: *const std::ffi::c_void,
            value: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn AXUIElementCopyElementAtPosition(
            application: *mut std::ffi::c_void,
            x: f32,
            y: f32,
            element: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementGetPid(element: *mut std::ffi::c_void, pid: *mut i32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetTypeID() -> usize;
        fn CFArrayGetCount(the_array: *const std::ffi::c_void) -> isize;
        fn CFArrayGetValueAtIndex(
            the_array: *const std::ffi::c_void,
            idx: isize,
        ) -> *const std::ffi::c_void;
        fn CFStringGetTypeID() -> usize;
        fn CFRetain(cf: *const std::ffi::c_void) -> *const std::ffi::c_void;
    }

    pub fn capture_now() -> Result<CaptureBundle> {
        unsafe { capture_now_inner() }
    }

    fn screencapture_cli() -> Result<(u32, u32, Vec<u8>)> {
        let path = std::env::temp_dir().join("snag-live-capture.png");
        let status = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x"])
            .arg(&path)
            .status()
            .map_err(|e| SnagError::from(e.to_string()))?;
        if !status.success() {
            return Err(SnagError::from(
                "Screen capture returned nothing. Grant Screen Recording to Snag in System Settings.",
            ));
        }
        let bytes = std::fs::read(&path)?;
        let _ = std::fs::remove_file(&path);
        let img = crate::image_util::decode_png(&bytes)?;
        Ok((img.width(), img.height(), img.into_raw()))
    }


    unsafe fn nsstring(s: *mut objc::runtime::Object) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let utf8: *const i8 = msg_send![s, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn frontmost_app() -> Option<String> {
        let ws: *mut objc::runtime::Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut objc::runtime::Object = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let name: *mut objc::runtime::Object = msg_send![app, localizedName];
        nsstring(name)
    }

    unsafe fn focused_window_title() -> Option<String> {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let focused_app_key = CFString::new("AXFocusedApplication");
        let mut app: *mut std::ffi::c_void = std::ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(
            system,
            focused_app_key.as_concrete_TypeRef() as _,
            &mut app,
        );
        CFRelease(system as _);
        if err != 0 || app.is_null() {
            return None;
        }
        let win_key = CFString::new("AXFocusedWindow");
        let mut win: *mut std::ffi::c_void = std::ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(app, win_key.as_concrete_TypeRef() as _, &mut win);
        CFRelease(app as _);
        if err != 0 || win.is_null() {
            return None;
        }
        let title_key = CFString::new("AXTitle");
        let mut title: *mut std::ffi::c_void = std::ptr::null_mut();
        let err =
            AXUIElementCopyAttributeValue(win, title_key.as_concrete_TypeRef() as _, &mut title);
        CFRelease(win as _);
        if err != 0 || title.is_null() {
            return None;
        }
        let cf = CFString::wrap_under_create_rule(title as _);
        Some(cf.to_string())
    }

    unsafe fn hit_element_at(pt: CGPoint) -> *mut std::ffi::c_void {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return std::ptr::null_mut();
        }
        let mut el: *mut std::ffi::c_void = std::ptr::null_mut();
        let err = AXUIElementCopyElementAtPosition(system, pt.x as f32, pt.y as f32, &mut el);
        CFRelease(system as _);
        if err != 0 || el.is_null() {
            std::ptr::null_mut()
        } else {
            el
        }
    }

    unsafe fn app_name_for_pid(pid: i32) -> Option<String> {
        let app: *mut objc::runtime::Object =
            msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return None;
        }
        let name: *mut objc::runtime::Object = msg_send![app, localizedName];
        nsstring(name)
    }

    /// App + window under the cursor, not whichever app happens to be focused (often Snag).
    unsafe fn target_at_cursor(pt: CGPoint) -> (Option<String>, Option<String>) {
        if !AXIsProcessTrusted() {
            return (frontmost_app(), focused_window_title());
        }
        let el = hit_element_at(pt);
        if el.is_null() {
            return (frontmost_app(), focused_window_title());
        }
        let mut pid: i32 = 0;
        let app = if AXUIElementGetPid(el, &mut pid) == 0 {
            app_name_for_pid(pid)
        } else {
            None
        };
        let mut title: Option<String> = None;
        let mut current = el;
        let mut owned: Vec<*mut std::ffi::c_void> = vec![el];
        for _ in 0..16 {
            if let Some(role) = ax_copy_string(current, "AXRole") {
                if role == "AXWindow" || role == "AXStandardWindow" {
                    title = ax_copy_string(current, "AXTitle");
                    break;
                }
            }
            let parent = ax_copy_attr(current, "AXParent");
            if parent.is_null() {
                break;
            }
            owned.push(parent);
            current = parent;
        }
        for p in owned {
            CFRelease(p as _);
        }
        (
            app.filter(|s| !s.is_empty()).or_else(|| unsafe { frontmost_app() }),
            title.filter(|s| !s.is_empty()).or_else(|| unsafe { focused_window_title() }),
        )
    }

    unsafe fn cursor_point() -> Result<CGPoint> {

        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| SnagError::from("Could not read cursor (event source)"))?;
        let event =
            CGEvent::new(source).map_err(|_| SnagError::from("Could not read cursor"))?;
        Ok(event.location())
    }

    unsafe fn display_for_point(pt: CGPoint) -> Result<u32> {
        let mut id: u32 = 0;
        let mut count: u32 = 0;
        let rc = CGGetDisplaysWithPoint(pt, 1, &mut id, &mut count);
        if rc != 0 || count == 0 {
            Ok(CGDisplay::main().id)
        } else {
            Ok(id)
        }
    }

    unsafe fn image_pixels(display: u32) -> Result<(u32, u32, Vec<u8>)> {
        let image = CGDisplayCreateImage(display);
        if image.is_null() {
            return screencapture_cli();
        }
        let width = CGImageGetWidth(image) as u32;
        let height = CGImageGetHeight(image) as u32;
        let bpr = CGImageGetBytesPerRow(image);
        let bpp = CGImageGetBitsPerPixel(image);
        let provider = CGImageGetDataProvider(image);
        if provider.is_null() {
            CGImageRelease(image);
            return Err(SnagError::from("Screen capture had no pixel buffer"));
        }
        let data_ref = CGDataProviderCopyData(provider);
        if data_ref.is_null() {
            CGImageRelease(image);
            return Err(SnagError::from("Screen capture had no pixel buffer"));
        }
        let cfdata = CFData::wrap_under_create_rule(data_ref as _);
        let raw = cfdata.bytes();
        let bpp_bytes = (bpp / 8).max(4);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for y in 0..height as usize {
            let row = y * bpr;
            for x in 0..width as usize {
                let i = row + x * bpp_bytes;
                if i + 3 >= raw.len() {
                    continue;
                }
                // macOS typically delivers BGRA
                let b = raw[i];
                let g = raw[i + 1];
                let r = raw[i + 2];
                let a = raw[i + 3];
                let o = (y * width as usize + x) * 4;
                rgba[o] = r;
                rgba[o + 1] = g;
                rgba[o + 2] = b;
                rgba[o + 3] = a;
            }
        }
        CGImageRelease(image);
        Ok((width, height, rgba))
    }

    fn copy_bgra(
        raw: &[u8],
        bpr: usize,
        bpp_bytes: usize,
        x0: usize,
        y0: usize,
        cw: usize,
        ch: usize,
        rgba: &mut [u8],
    ) {
        for y in 0..ch {
            let row = (y0 + y) * bpr;
            for x in 0..cw {
                let i = row + (x0 + x) * bpp_bytes;
                if i + 3 >= raw.len() {
                    continue;
                }
                let o = (y * cw + x) * 4;
                rgba[o] = raw[i + 2];
                rgba[o + 1] = raw[i + 1];
                rgba[o + 2] = raw[i];
                rgba[o + 3] = raw[i + 3];
            }
        }
    }

    struct DisplayGrab {
        crop_w: u32,
        crop_h: u32,
        crop_rgba: Vec<u8>,
        crop_cx: i32,
        crop_cy: i32,
        full_w: u32,
        full_h: u32,
        full_rgba: Vec<u8>,
        full_cx: i32,
        full_cy: i32,
    }

    fn downsample_bgra(
        raw: &[u8],
        bpr: usize,
        bpp_bytes: usize,
        width: i32,
        height: i32,
        max_width: u32,
        px: i32,
        py: i32,
    ) -> (u32, u32, Vec<u8>, i32, i32) {
        let src_w = width.max(1) as u32;
        let src_h = height.max(1) as u32;
        let scale = if src_w > max_width {
            max_width as f32 / src_w as f32
        } else {
            1.0
        };
        let tw = ((src_w as f32) * scale).round().max(1.0) as u32;
        let th = ((src_h as f32) * scale).round().max(1.0) as u32;
        let mut rgba = vec![0u8; tw as usize * th as usize * 4];
        for y in 0..th {
            let sy = ((y as f32 / scale) as i32).clamp(0, height - 1) as usize;
            for x in 0..tw {
                let sx = ((x as f32 / scale) as i32).clamp(0, width - 1) as usize;
                let i = sy * bpr + sx * bpp_bytes;
                if i + 3 >= raw.len() {
                    continue;
                }
                let o = ((y * tw + x) * 4) as usize;
                rgba[o] = raw[i + 2];
                rgba[o + 1] = raw[i + 1];
                rgba[o + 2] = raw[i];
                rgba[o + 3] = raw[i + 3];
            }
        }
        let (cx, cy) = image_util::scale_cursor(px, py, src_w, src_h, tw, th);
        (tw, th, rgba, cx, cy)
    }

    fn grab_from_rgba(
        w: u32,
        h: u32,
        rgba: Vec<u8>,
        px: i32,
        py: i32,
        radius: i32,
    ) -> DisplayGrab {
        let x0 = (px - radius).max(0);
        let y0 = (py - radius).max(0);
        let x1 = (px + radius).min(w as i32);
        let y1 = (py + radius).min(h as i32);
        let cw = (x1 - x0).max(1) as usize;
        let ch = (y1 - y0).max(1) as usize;
        let mut crop = vec![0u8; cw * ch * 4];
        for y in 0..ch {
            let src = ((y0 as usize + y) * w as usize + x0 as usize) * 4;
            let dst = y * cw * 4;
            if src + cw * 4 <= rgba.len() {
                crop[dst..dst + cw * 4].copy_from_slice(&rgba[src..src + cw * 4]);
            }
        }
        let (fw, fh, full) =
            image_util::downsample_rgba(w, h, &rgba, image_util::FULL_MAX_WIDTH);
        let (fcx, fcy) = image_util::scale_cursor(px, py, w, h, fw, fh);
        DisplayGrab {
            crop_w: cw as u32,
            crop_h: ch as u32,
            crop_rgba: crop,
            crop_cx: px - x0,
            crop_cy: py - y0,
            full_w: fw,
            full_h: fh,
            full_rgba: full,
            full_cx: fcx,
            full_cy: fcy,
        }
    }

    unsafe fn image_grab(
        display: u32,
        px: i32,
        py: i32,
        radius: i32,
    ) -> Result<DisplayGrab> {
        let image = CGDisplayCreateImage(display);
        if image.is_null() {
            let (w, h, rgba) = screencapture_cli()?;
            return Ok(grab_from_rgba(w, h, rgba, px, py, radius));
        }
        let width = CGImageGetWidth(image) as i32;
        let height = CGImageGetHeight(image) as i32;
        let bpr = CGImageGetBytesPerRow(image);
        let bpp = CGImageGetBitsPerPixel(image);
        let provider = CGImageGetDataProvider(image);
        if provider.is_null() {
            CGImageRelease(image);
            return Err(SnagError::from("Screen capture had no pixel buffer"));
        }
        let data_ref = CGDataProviderCopyData(provider);
        if data_ref.is_null() {
            CGImageRelease(image);
            return Err(SnagError::from("Screen capture had no pixel buffer"));
        }
        let cfdata = CFData::wrap_under_create_rule(data_ref as _);
        let raw = cfdata.bytes();
        let bpp_bytes = (bpp / 8).max(4);
        let x0 = (px - radius).max(0);
        let y0 = (py - radius).max(0);
        let x1 = (px + radius).min(width);
        let y1 = (py + radius).min(height);
        let cw = (x1 - x0).max(1) as usize;
        let ch = (y1 - y0).max(1) as usize;
        let mut rgba = vec![0u8; cw * ch * 4];
        copy_bgra(raw, bpr, bpp_bytes, x0 as usize, y0 as usize, cw, ch, &mut rgba);
        let (fw, fh, full, fcx, fcy) = downsample_bgra(
            raw,
            bpr,
            bpp_bytes,
            width,
            height,
            image_util::FULL_MAX_WIDTH,
            px,
            py,
        );
        CGImageRelease(image);
        Ok(DisplayGrab {
            crop_w: cw as u32,
            crop_h: ch as u32,
            crop_rgba: rgba,
            crop_cx: px - x0,
            crop_cy: py - y0,
            full_w: fw,
            full_h: fh,
            full_rgba: full,
            full_cx: fcx,
            full_cy: fcy,
        })
    }

    unsafe fn capture_now_inner() -> Result<CaptureBundle> {

        let pt = cursor_point()?;
        let display = display_for_point(pt)?;
        let bounds = CGDisplayBounds(display);
        let pix_w = CGDisplayPixelsWide(display).max(1) as f64;
        let pix_h = CGDisplayPixelsHigh(display).max(1) as f64;
        let scale_x = pix_w / bounds.size.width.max(1.0);
        let scale_y = pix_h / bounds.size.height.max(1.0);
        // Cocoa / CGEvent: origin bottom-left. Image pixels: origin top-left.
        let rel_x = pt.x - bounds.origin.x;
        let rel_y_from_bottom = pt.y - bounds.origin.y;
        let px = (rel_x * scale_x).clamp(0.0, pix_w - 1.0);
        let py = ((bounds.size.height - rel_y_from_bottom) * scale_y).clamp(0.0, pix_h - 1.0);

        let grab = image_grab(display, px as i32, py as i32, crate::image_util::CROP_RADIUS)?;
        let mut crop = image_util::from_rgba(grab.crop_w, grab.crop_h, grab.crop_rgba);
        image_util::mark_cursor(&mut crop, grab.crop_cx, grab.crop_cy);
        let crop_png = image_util::encode_png(&crop)?;
        let mut full = image_util::from_rgba(grab.full_w, grab.full_h, grab.full_rgba);
        image_util::mark_cursor(&mut full, grab.full_cx, grab.full_cy);
        let full_png = image_util::encode_png(&full)?;

        let (app, title) = target_at_cursor(pt);
        Ok(CaptureBundle {
            full_png,
            crop_png,
            cursor_x: px,
            cursor_y: py,
            source_app: app,
            window_title: title,
            fixture_id: None,
            document_text: document_text_at_cursor(pt),
        })
    }

    const AX_PARENT_WALKS: usize = 12;
    const AX_MAX_CHILD_NODES: usize = 280;
    const AX_MAX_CHARS: usize = 80_000;
    const AX_LONG_SELECTED: usize = 80;

    fn document_text_at_cursor(pt: CGPoint) -> Option<String> {
        // Never crash capture if Accessibility is off or AX hangs-ish.
        if !unsafe { AXIsProcessTrusted() } {
            return None;
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { scrape_ax(pt) }))
            .ok()
            .flatten()
    }

    unsafe fn ax_copy_attr(element: *mut std::ffi::c_void, attr: &str) -> *mut std::ffi::c_void {
        if element.is_null() {
            return std::ptr::null_mut();
        }
        let key = CFString::new(attr);
        let mut value: *mut std::ffi::c_void = std::ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(
            element,
            key.as_concrete_TypeRef() as _,
            &mut value,
        );
        if err != 0 {
            std::ptr::null_mut()
        } else {
            value
        }
    }

    unsafe fn ax_copy_string(element: *mut std::ffi::c_void, attr: &str) -> Option<String> {
        let value = ax_copy_attr(element, attr);
        if value.is_null() {
            return None;
        }
        if CFGetTypeID(value as _) as usize != CFStringGetTypeID() {
            CFRelease(value as _);
            return None;
        }
        let cf = CFString::wrap_under_create_rule(value as _);
        let s = cf.to_string();
        if s.trim().is_empty() {
            None
        } else {
            Some(s)
        }
    }

    unsafe fn collect_strings(
        element: *mut std::ffi::c_void,
        fragments: &mut Vec<String>,
        selected: &mut Vec<String>,
        total_chars: &mut usize,
    ) {
        if *total_chars >= AX_MAX_CHARS {
            return;
        }
        for attr in [
            "AXValue",
            "AXSelectedText",
            "AXDescription",
            "AXTitle",
            "AXDocument",
        ] {
            if let Some(s) = ax_copy_string(element, attr) {
                *total_chars = total_chars.saturating_add(s.len());
                if attr == "AXSelectedText" {
                    selected.push(s.clone());
                }
                fragments.push(s);
                if *total_chars >= AX_MAX_CHARS {
                    return;
                }
            }
        }
    }

    unsafe fn ax_children_retained(element: *mut std::ffi::c_void) -> Vec<*mut std::ffi::c_void> {
        let arr = ax_copy_attr(element, "AXChildren");
        if arr.is_null() {
            return Vec::new();
        }
        if CFGetTypeID(arr as _) as usize != CFArrayGetTypeID() {
            CFRelease(arr as _);
            return Vec::new();
        }
        let count = CFArrayGetCount(arr as _);
        let mut out = Vec::new();
        for i in 0..count {
            let item = CFArrayGetValueAtIndex(arr as _, i) as *mut std::ffi::c_void;
            if !item.is_null() {
                CFRetain(item as _);
                out.push(item);
            }
        }
        CFRelease(arr as _);
        out
    }

    fn is_chrome(s: &str) -> bool {
        let l = s.trim().to_lowercase();
        l.starts_with("filter by unread")
            || matches!(
                l.as_str(),
                "0 unread"
                    | "unread"
                    | "search"
                    | "messages"
                    | "add canvas"
                    | "files & links"
                    | "files and links"
            )
    }

    fn compose_document(selected: &[String], fragments: &[String]) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut ordered: Vec<&String> = selected.iter().chain(fragments.iter()).collect();
        ordered.sort_by_key(|s| std::cmp::Reverse(s.len()));
        for raw in ordered {
            let trimmed = raw.trim();
            if trimmed.len() < 4 || is_chrome(trimmed) {
                continue;
            }
            let key = trimmed.to_lowercase();
            if seen.iter().any(|s| s == &key || s.contains(&key)) {
                continue;
            }
            seen.retain(|s| !key.contains(s.as_str()));
            seen.push(key);
            parts.push(trimmed.to_string());
            if parts.iter().map(|s| s.len()).sum::<usize>() >= AX_MAX_CHARS {
                break;
            }
        }
        if parts.is_empty() {
            return None;
        }
        let mut joined = parts.join("\n\n");
        if joined.len() > AX_MAX_CHARS {
            joined.truncate(AX_MAX_CHARS);
        }
        let t = joined.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }

    unsafe fn scrape_ax(pt: CGPoint) -> Option<String> {
        use std::collections::VecDeque;

        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let mut el: *mut std::ffi::c_void = std::ptr::null_mut();
        let err = AXUIElementCopyElementAtPosition(system, pt.x as f32, pt.y as f32, &mut el);
        CFRelease(system as _);
        if err != 0 || el.is_null() {
            return None;
        }

        let mut fragments: Vec<String> = Vec::new();
        let mut selected: Vec<String> = Vec::new();
        let mut total_chars: usize = 0;

        let mut current = el;
        let mut parent_chain: Vec<*mut std::ffi::c_void> = vec![el];
        for _ in 0..AX_PARENT_WALKS {
            collect_strings(current, &mut fragments, &mut selected, &mut total_chars);
            let parent = ax_copy_attr(current, "AXParent");
            if parent.is_null() {
                break;
            }
            parent_chain.push(parent);
            current = parent;
        }

        let mut start = *parent_chain.last().unwrap_or(&el);
        for &node in parent_chain.iter().rev() {
            if let Some(role) = ax_copy_string(node, "AXRole") {
                if role == "AXWindow" || role == "AXStandardWindow" {
                    start = node;
                    break;
                }
            }
        }
        let mut queue: VecDeque<*mut std::ffi::c_void> = VecDeque::new();
        let mut owned_children: Vec<*mut std::ffi::c_void> = Vec::new();
        for k in ax_children_retained(start) {
            queue.push_back(k);
            owned_children.push(k);
        }

        let mut nodes = 0usize;
        while let Some(node) = queue.pop_front() {
            if nodes >= AX_MAX_CHILD_NODES || total_chars >= AX_MAX_CHARS {
                break;
            }
            nodes += 1;
            collect_strings(node, &mut fragments, &mut selected, &mut total_chars);
            if nodes >= AX_MAX_CHILD_NODES || total_chars >= AX_MAX_CHARS {
                break;
            }
            for k in ax_children_retained(node) {
                if owned_children.len() >= AX_MAX_CHILD_NODES {
                    CFRelease(k as _);
                    continue;
                }
                queue.push_back(k);
                owned_children.push(k);
            }
        }

        for p in parent_chain {
            CFRelease(p as _);
        }
        for c in owned_children {
            CFRelease(c as _);
        }

        compose_document(&selected, &fragments)
    }
}

#[cfg(target_os = "macos")]
pub fn capture_screen() -> Result<CaptureBundle> {
    macos::capture_now()
}

#[cfg(not(target_os = "macos"))]
pub fn capture_screen() -> Result<CaptureBundle> {
    Err(SnagError::from(
        "Live capture is macOS-only. Enable demo mode to use fixtures.",
    ))
}

pub fn capture(demo: bool, fixture: &str) -> Result<CaptureBundle> {
    if demo {
        capture_demo(fixture)
    } else {
        capture_screen()
    }
}
