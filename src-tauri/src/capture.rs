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

    unsafe fn capture_now_inner() -> Result<CaptureBundle> {
        let pt = cursor_point()?;
        let display = display_for_point(pt)?;
        let bounds = CGDisplayBounds(display);
        let (width, height, rgba) = image_pixels(display)?;
        let scale_x = width as f64 / bounds.size.width.max(1.0);
        let scale_y = height as f64 / bounds.size.height.max(1.0);
        // Cocoa / CGEvent: origin bottom-left. Image pixels: origin top-left.
        let rel_x = pt.x - bounds.origin.x;
        let rel_y_from_bottom = pt.y - bounds.origin.y;
        let px = (rel_x * scale_x).clamp(0.0, (width.saturating_sub(1)) as f64);
        let py = ((bounds.size.height - rel_y_from_bottom) * scale_y)
            .clamp(0.0, (height.saturating_sub(1)) as f64);

        let mut img = image_util::from_rgba(width, height, rgba);
        image_util::mark_cursor(&mut img, px as i32, py as i32);
        let crop = image_util::crop_around(&img, px as i32, py as i32);
        let full_png = image_util::encode_png(&img)?;
        let crop_png = image_util::encode_png(&crop)?;

        Ok(CaptureBundle {
            full_png,
            crop_png,
            cursor_x: px,
            cursor_y: py,
            source_app: frontmost_app(),
            window_title: focused_window_title(),
            fixture_id: None,
            document_text: document_text_at_cursor(pt),
        })
    }

    const AX_PARENT_WALKS: usize = 12;
    const AX_MAX_CHILD_NODES: usize = 80;
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

    fn compose_document(selected: &[String], fragments: &[String]) -> Option<String> {
        let longest_sel = selected.iter().max_by_key(|s| s.len());
        let longest = fragments.iter().max_by_key(|s| s.len());
        let mut parts: Vec<&str> = Vec::new();
        if let Some(sel) = longest_sel {
            if sel.len() >= AX_LONG_SELECTED {
                parts.push(sel.as_str());
            }
        }
        if let Some(long) = longest {
            let dup = parts.first().map(|p| *p == long.as_str()).unwrap_or(false);
            if !dup {
                parts.push(long.as_str());
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

        let start = *parent_chain.last().unwrap_or(&el);
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
