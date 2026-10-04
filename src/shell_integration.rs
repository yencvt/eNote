//! Windows Explorer "Open with eNote" right-click context menu integration, mirroring the
//! option Notepad++'s installer offers under the same name. Registers a *per-user* entry
//! (no admin rights needed) under `HKEY_CURRENT_USER\Software\Classes\*\shell\...`, which
//! Windows transparently merges into `HKEY_CLASSES_ROOT` so it shows up for every file
//! type's right-click menu.
//!
//! No-ops (returning an explanatory error) on non-Windows targets, so callers don't need
//! to sprinkle `#[cfg(windows)]` everywhere themselves.

#[cfg(target_os = "windows")]
mod imp {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    const MENU_KEY_PATH: &str = r"Software\Classes\*\shell\Open with eNote";
    const MENU_LABEL: &str = "Open with eNote";

    fn exe_path() -> Result<String, String> {
        std::env::current_exe()
            .map_err(|e| format!("Couldn't locate the running executable: {e}"))
            .map(|p| p.display().to_string())
    }

    /// Whether the context menu entry is currently registered for the current user.
    pub fn is_registered() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(MENU_KEY_PATH)
            .is_ok()
    }

    /// Adds the "Open with eNote" entry to the right-click menu of every file, launching
    /// this same executable with the clicked file's path as its only argument.
    pub fn register() -> Result<(), String> {
        let exe = exe_path()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        let (menu_key, _) = hkcu
            .create_subkey(MENU_KEY_PATH)
            .map_err(|e| format!("Failed to create registry key: {e}"))?;
        menu_key
            .set_value("", &MENU_LABEL)
            .map_err(|e| format!("Failed to set menu label: {e}"))?;
        menu_key
            .set_value("Icon", &format!("\"{exe}\",0"))
            .map_err(|e| format!("Failed to set menu icon: {e}"))?;

        let (command_key, _) = menu_key
            .create_subkey("command")
            .map_err(|e| format!("Failed to create command registry key: {e}"))?;
        command_key
            .set_value("", &format!("\"{exe}\" \"%1\""))
            .map_err(|e| format!("Failed to set command registry value: {e}"))?;

        Ok(())
    }

    /// Removes the context menu entry added by [`register`].
    pub fn unregister() -> Result<(), String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .delete_subkey_all(MENU_KEY_PATH)
            .map_err(|e| format!("Failed to remove registry key: {e}"))
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub fn is_registered() -> bool {
        false
    }

    pub fn register() -> Result<(), String> {
        Err("Explorer context menu integration is only available on Windows".to_string())
    }

    pub fn unregister() -> Result<(), String> {
        Err("Explorer context menu integration is only available on Windows".to_string())
    }
}

pub use imp::{is_registered, register, unregister};

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    // Exercises the real HKCU registry (no admin rights needed), cleaning up afterwards.
    // Safe to run repeatedly/in parallel with itself since it always ends by unregistering.
    #[test]
    fn register_then_unregister_round_trips() {
        unregister().ok(); // start from a clean slate in case a previous run was aborted
        assert!(!is_registered());

        register().expect("register should succeed under the current user's HKCU");
        assert!(is_registered());

        unregister().expect("unregister should succeed");
        assert!(!is_registered());
    }
}
