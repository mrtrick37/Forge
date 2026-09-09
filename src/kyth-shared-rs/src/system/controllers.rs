//! Port of `kyth_shared.system.controllers` — pure controller detection.
//! lsusb → vid/pid → PlayStation/Xbox/Nintendo + lsmod + /dev/input/by-id

use std::fs;

fn command_stdout(cmd: &str, args: &[&str], timeout_secs: u64) -> String {
    let mut argv = vec![cmd.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    crate::system::process::run_bounded(&argv, std::time::Duration::from_secs(timeout_secs))
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        .unwrap_or_default()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ControllersDetect {
    pub usb_controllers: Vec<(String, String)>,
    pub input_nodes: Vec<String>,
    pub xone_dongle: bool,
    pub xone_loaded: bool,
    pub xpadneo_loaded: bool,
    pub hid_ps_loaded: bool,
    pub dualsense_found: bool,
    pub ds4_found: bool,
    pub switch_pro_found: bool,
    pub dualsensectl_out: String,
    pub secure_boot: bool,
    pub jstest_available: bool,
}

#[derive(Default)]
struct UsbControllerParse {
    controllers: Vec<(String, String)>,
    xone_dongle: bool,
    dualsense_found: bool,
    ds4_found: bool,
    switch_pro_found: bool,
}

fn parse_usb_controllers(usb_text: &str) -> UsbControllerParse {
    let mut parsed = UsbControllerParse::default();
    for line in usb_text.lines() {
        let Some(id) = line
            .split_once("ID ")
            .and_then(|(_, rest)| rest.split_whitespace().next())
        else {
            continue;
        };
        let Some((vid, pid)) = id.split_once(':') else {
            continue;
        };
        let vid = vid.to_ascii_lowercase();
        let pid = pid.to_ascii_lowercase();
        let description = line
            .split_once(id)
            .map(|(_, rest)| rest.trim())
            .unwrap_or_default();
        let label = match vid.as_str() {
            "045e" => "Xbox",
            "054c" => "PlayStation",
            "057e" => "Nintendo",
            "2dc8" => "8BitDo",
            "0f0d" => "HORI",
            "28de" => "Valve",
            "20d6" => "PowerA",
            "0e6f" => "PDP",
            _ => continue,
        };
        if vid == "045e" && matches!(pid.as_str(), "02e6" | "02fe") {
            parsed.xone_dongle = true;
            parsed
                .controllers
                .push(("Xbox Wireless USB Dongle".into(), "xbox_dongle".into()));
        } else if vid == "054c" && matches!(pid.as_str(), "0ce6" | "0df2") {
            parsed.dualsense_found = true;
            parsed
                .controllers
                .push(("PlayStation 5 DualSense".into(), "dualsense".into()));
        } else if vid == "054c" && matches!(pid.as_str(), "05c4" | "09cc" | "0ba0") {
            parsed.ds4_found = true;
            parsed
                .controllers
                .push(("PlayStation 4 DualShock 4".into(), "ds4".into()));
        } else if vid == "057e" && pid == "2009" {
            parsed.switch_pro_found = true;
            parsed
                .controllers
                .push(("Nintendo Switch Pro Controller".into(), "switch_pro".into()));
        } else {
            parsed.controllers.push((
                if description.is_empty() {
                    format!("{label} controller")
                } else {
                    description.into()
                },
                "generic".into(),
            ));
        }
    }
    parsed
}

pub fn detect_controllers() -> ControllersDetect {
    let usb_text = command_stdout("lsusb", &[], 6);
    let lsmod_text = command_stdout("lsmod", &[], 4);
    let parsed = parse_usb_controllers(&usb_text);
    let input_nodes = fs::read_dir("/dev/input/by-id")
        .ok()
        .map(|rd| {
            let mut v: Vec<String> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| {
                    let l = n.to_lowercase();
                    l.contains("joystick") || l.contains("gamepad") || l.contains("controller")
                })
                .collect();
            v.sort();
            v
        })
        .unwrap_or_default();

    let secure_boot = fs::read_dir("/sys/firmware/efi/efivars")
        .ok()
        .and_then(|rd| {
            for e in rd.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("SecureBoot-") {
                    if let Ok(data) = fs::read(e.path()) {
                        return Some(data.len() >= 5 && data[4] == 1);
                    }
                }
            }
            None
        })
        .unwrap_or(false);

    let modules = lsmod_text.to_lowercase().replace('-', "_");
    let dualsensectl_out = if parsed.dualsense_found && which("dualsensectl") {
        command_stdout("dualsensectl", &["status", "0"], 3)
    } else {
        String::new()
    };
    ControllersDetect {
        usb_controllers: parsed.controllers,
        input_nodes,
        xone_dongle: parsed.xone_dongle,
        xone_loaded: modules.contains("xone_hid"),
        xpadneo_loaded: modules.contains("xpadneo"),
        hid_ps_loaded: modules.contains("hid_playstation"),
        dualsense_found: parsed.dualsense_found,
        ds4_found: parsed.ds4_found,
        switch_pro_found: parsed.switch_pro_found,
        dualsensectl_out,
        secure_boot,
        jstest_available: which("jstest-gtk"),
    }
}

fn which(command: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(command).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_known_usb_controller_variants() {
        let parsed = parse_usb_controllers(concat!(
            "Bus 001 Device 002: ID 054c:0ce6 Sony DualSense\n",
            "Bus 001 Device 003: ID 054c:05c4 Sony DualShock\n",
            "Bus 001 Device 004: ID 057e:2009 Nintendo Pro\n",
            "Bus 001 Device 005: ID 1d6b:0002 ignored\n",
        ));
        assert!(parsed.dualsense_found);
        assert!(parsed.ds4_found);
        assert!(parsed.switch_pro_found);
        assert_eq!(parsed.controllers.len(), 3);
    }

    #[test]
    fn detect_returns_struct() {
        let d = detect_controllers();
        // Just verify it doesn't panic and fields are bool/vec
        let _ = d.xone_loaded;
        assert!(d.usb_controllers.len() <= 20);
    }
}
