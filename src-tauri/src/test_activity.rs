//! Scoped macOS process activity for the dedicated hidden native UI test.
//! It prevents App Nap from suspending the test journey without disabling
//! idle system sleep; production browsing does not use this activity.

#[cfg(target_os = "macos")]
pub struct ScopedTestActivity {
    process_info: objc2::rc::Retained<objc2_foundation::NSProcessInfo>,
    token: objc2::rc::Retained<
        objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>,
    >,
}

#[cfg(target_os = "macos")]
// NSProcessInfo is documented thread-safe and the opaque activity token is
// retained until Drop, where the paired endActivity call is made.
unsafe impl Send for ScopedTestActivity {}

#[cfg(target_os = "macos")]
impl ScopedTestActivity {
    pub fn begin() -> Self {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

        let process_info = NSProcessInfo::processInfo();
        let reason = NSString::from_str("Hrungnir native UI test journey");
        let token = process_info.beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
            &reason,
        );
        Self { process_info, token }
    }
}

#[cfg(target_os = "macos")]
impl Drop for ScopedTestActivity {
    fn drop(&mut self) {
        unsafe { self.process_info.endActivity(&self.token) };
    }
}

#[cfg(not(target_os = "macos"))]
pub struct ScopedTestActivity;

#[cfg(not(target_os = "macos"))]
impl ScopedTestActivity {
    pub fn begin() -> Self {
        Self
    }
}
