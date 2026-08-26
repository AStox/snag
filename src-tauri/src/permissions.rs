use crate::models::PermissionStatus;

#[cfg(target_os = "macos")]
mod macos {
    use objc::{class, msg_send, sel, sel_impl};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    }

    pub fn screen() -> &'static str {
        unsafe {
            if CGPreflightScreenCaptureAccess() {
                "granted"
            } else {
                "denied"
            }
        }
    }

    pub fn request_screen() -> &'static str {
        unsafe {
            let _ = CGRequestScreenCaptureAccess();
            screen()
        }
    }

    pub fn accessibility() -> &'static str {
        unsafe {
            if AXIsProcessTrusted() {
                "granted"
            } else {
                "denied"
            }
        }
    }

    pub fn request_accessibility() -> &'static str {
        use core_foundation::base::TCFType;
        use core_foundation::boolean::CFBoolean;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;

        unsafe {
            let key = CFString::new("AXTrustedCheckOptionPrompt");
            let val = CFBoolean::from(true);
            let dict: CFDictionary<CFString, CFBoolean> =
                CFDictionary::from_CFType_pairs(&[(key, val)]);
            let _ = AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as *const _);
            accessibility()
        }
    }

    pub fn microphone() -> &'static str {
        unsafe {
            let status: i64 = msg_send![
                class!(AVCaptureDevice),
                authorizationStatusForMediaType: av_media_audio()
            ];
            match status {
                3 => "granted", // Authorized
                2 => "denied",  // Denied
                1 => "denied",  // Restricted
                _ => "unknown", // NotDetermined = 0
            }
        }
    }

    #[allow(dead_code)]
    pub fn request_microphone() -> &'static str {
        unsafe {
            let _: () = msg_send![
                class!(AVCaptureDevice),
                requestAccessForMediaType: av_media_audio()
                completionHandler: request_mic_handler()
            ];
        }
        microphone()
    }

    fn request_mic_handler() -> *mut objc::runtime::Object {
        std::ptr::null_mut()
    }

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    unsafe fn av_media_audio() -> *mut objc::runtime::Object {
        use cocoa::base::nil;
        use cocoa::foundation::NSString;
        NSString::alloc(nil).init_str("soun")
    }
}

#[cfg(target_os = "macos")]
pub fn status() -> PermissionStatus {
    PermissionStatus {
        screen: macos::screen().into(),
        microphone: macos::microphone().into(),
        accessibility: macos::accessibility().into(),
        platform: "macos".into(),
    }
}

#[cfg(target_os = "macos")]
pub fn request() -> PermissionStatus {
    let _ = macos::request_screen();
    // Prompt for Accessibility so Grain-length docs can actually be read.
    let _ = macos::request_accessibility();
    // Microphone is optional and not required for the default (screen-only) flow.
    let _ = macos::microphone();
    status()
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> PermissionStatus {
    PermissionStatus {
        screen: "unknown".into(),
        microphone: "unknown".into(),
        accessibility: "unknown".into(),
        platform: "other".into(),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request() -> PermissionStatus {
    status()
}
