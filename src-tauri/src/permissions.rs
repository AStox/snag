use crate::models::PermissionStatus;

#[cfg(target_os = "macos")]
mod macos {
    use objc::{class, msg_send, sel, sel_impl};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
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
        platform: "macos".into(),
    }
}

#[cfg(target_os = "macos")]
pub fn request() -> PermissionStatus {
    let _ = macos::request_screen();
    let _ = macos::request_microphone();
    status()
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> PermissionStatus {
    PermissionStatus {
        screen: "unknown".into(),
        microphone: "unknown".into(),
        platform: "other".into(),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request() -> PermissionStatus {
    status()
}
