use std::process::Child;
use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::devices::HotplugCmd;

/// (location_id, mode, label)
pub type DeviceSnapshot = (String, String, String);

pub struct AppState {
    pub selected_device: Mutex<Option<String>>,
    pub busy: Mutex<bool>,
    pub last_devices: Mutex<Vec<DeviceSnapshot>>,
    pub hotplug_tx: Mutex<Option<mpsc::UnboundedSender<HotplugCmd>>>,
    pub logcat_child: Mutex<Option<Child>>,
    pub logcat_lines: Mutex<Vec<String>>,
    pub logcat_generation: Mutex<u64>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            selected_device: Mutex::new(None),
            busy: Mutex::new(false),
            last_devices: Mutex::new(Vec::new()),
            hotplug_tx: Mutex::new(None),
            logcat_child: Mutex::new(None),
            logcat_lines: Mutex::new(Vec::new()),
            logcat_generation: Mutex::new(0),
        }
    }
}
