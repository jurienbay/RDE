use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum WmAction {
    Exec(String),
    TogglePersonality,
    LockSession,
    SwitchWorkspace(u32),
    Module(String),
    Nop,
}

#[derive(Debug, Clone)]
pub struct MenuEntryConfig {
    pub label: String,
    pub action: WmAction,
}

#[derive(Debug, Clone)]
pub struct KeyBindingConfig {
    pub x11_keycode: u32,
    pub require_ctrl: bool,
    pub require_alt: bool,
    pub require_super: bool,
    pub action: WmAction,
}

#[derive(Debug, Clone)]
pub struct WmConfig {
    pub default_personality: String,
    pub edge_pan_cooldown_ms: u128,
    pub sloppy_focus: bool,
    pub root_menu: Vec<MenuEntryConfig>,
    pub keybindings: Vec<KeyBindingConfig>,
    pub startup_modules: Vec<String>,
}

impl Default for WmConfig {
    fn default() -> Self {
        Self {
            default_personality: "CDE".to_string(),
            edge_pan_cooldown_ms: 350,
            sloppy_focus: true,
            root_menu: vec![
                MenuEntryConfig { label: "TERM".into(), action: WmAction::Exec("foot".into()) },
                MenuEntryConfig { label: "FILE".into(), action: WmAction::Exec("pcmanfm".into()) },
                MenuEntryConfig { label: "LOCK".into(), action: WmAction::LockSession },
                MenuEntryConfig { label: "EXIT".into(), action: WmAction::Nop },
            ],
            keybindings: vec![
                KeyBindingConfig {
                    x11_keycode: 50 + 8, // M
                    require_ctrl: true,
                    require_alt: true,
                    require_super: false,
                    action: WmAction::TogglePersonality,
                },
                KeyBindingConfig {
                    x11_keycode: 37 + 8, // K
                    require_ctrl: true,
                    require_alt: true,
                    require_super: false,
                    action: WmAction::LockSession,
                },
            ],
            startup_modules: vec![],
        }
    }
}

impl WmConfig {
    pub fn load() -> Self {
        let mut cfg = Self::default();
        let path = Self::config_path();
        let Ok(contents) = fs::read_to_string(&path) else {
            println!("Config: No config found at {:?}, using built-in defaults.", path);
            return cfg;
        };

        println!("Config: Loaded configuration from {:?}", path);
        let mut custom_menu = Vec::new();
        let mut custom_keys = Vec::new();
        let mut in_start_func = false;

        for raw_line in contents.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Handle FVWM `+ I ...` function continuation lines
            if let Some(rest) = line.strip_prefix('+') {
                if in_start_func {
                    let tokens: Vec<&str> = rest.split_whitespace().collect();
                    if tokens.len() >= 3 && tokens[0] == "I" && tokens[1].eq_ignore_ascii_case("Module") {
                        cfg.startup_modules.push(tokens[2].to_string());
                    }
                }
                continue;
            }
            in_start_func = false;

            let tokens = tokenize_fvwm_line(line);
            if tokens.is_empty() {
                continue;
            }

            match tokens[0].to_ascii_lowercase().as_str() {
                "personality" if tokens.len() >= 2 => {
                    cfg.default_personality = tokens[1].to_ascii_uppercase();
                }
                "edgeresistance" if tokens.len() >= 2 => {
                    if let Ok(ms) = tokens[1].parse::<u128>() {
                        cfg.edge_pan_cooldown_ms = ms;
                    }
                }
                "focusstyle" if tokens.len() >= 2 => {
                    cfg.sloppy_focus = !tokens[1].eq_ignore_ascii_case("ClickToFocus");
                }
                "addtofunc" if tokens.len() >= 2 && tokens[1].eq_ignore_ascii_case("StartFunction") => {
                    in_start_func = true;
                }
                "addtomenu" if tokens.len() >= 4 && tokens[1].eq_ignore_ascii_case("RootMenu") => {
                    let label = tokens[2].clone();
                    let action = parse_action(&tokens[3..]);
                    custom_menu.push(MenuEntryConfig { label, action });
                }
                "key" if tokens.len() >= 5 => {
                    if let Some(x11_keycode) = key_name_to_x11(&tokens[1]) {
                        let mods = tokens[3].to_ascii_uppercase();
                        let action = parse_action(&tokens[4..]);
                        custom_keys.push(KeyBindingConfig {
                            x11_keycode,
                            require_ctrl: mods.contains('C'),
                            require_alt: mods.contains('M'),
                            require_super: mods.contains('4') || mods.contains('S'),
                            action,
                        });
                    }
                }
                _ => {}
            }
        }

        if !custom_menu.is_empty() {
            cfg.root_menu = custom_menu;
        }
        if !custom_keys.is_empty() {
            cfg.keybindings = custom_keys;
        }
        cfg
    }

    fn config_path() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("cde-fvwm/config")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config/cde-fvwm/config")
        } else {
            PathBuf::from("fvwm2rc")
        }
    }
}

fn tokenize_fvwm_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;

    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_action(tokens: &[String]) -> WmAction {
    if tokens.is_empty() {
        return WmAction::Nop;
    }
    match tokens[0].to_ascii_lowercase().as_str() {
        "exec" if tokens.len() >= 2 => WmAction::Exec(tokens[1..].join(" ")),
        "togglepersonality" => WmAction::TogglePersonality,
        "locksession" => WmAction::LockSession,
        "gotodesk" if tokens.len() >= 2 => {
            WmAction::SwitchWorkspace(tokens[1].parse::<u32>().unwrap_or(0).min(3))
        }
        "module" if tokens.len() >= 2 => WmAction::Module(tokens[1].clone()),
        _ => WmAction::Nop,
    }
}

fn key_name_to_x11(name: &str) -> Option<u32> {
    let evdev = match name.to_ascii_uppercase().as_str() {
        "Q" => 16, "W" => 17, "E" => 18, "R" => 19, "T" => 20, "Y" => 21, "U" => 22, "I" => 23, "O" => 24, "P" => 25,
        "A" => 30, "S" => 31, "D" => 32, "F" => 33, "G" => 34, "H" => 35, "J" => 36, "K" => 37, "L" => 38,
        "Z" => 44, "X" => 45, "C" => 46, "V" => 47, "B" => 48, "N" => 49, "M" => 50,
        "RETURN" | "ENTER" => 28,
        "SPACE" => 57,
        "TAB" => 15,
        "1" => 2, "2" => 3, "3" => 4, "4" => 5,
        _ => return None,
    };
    Some(evdev + 8)
}
