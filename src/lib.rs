//! Smooth F5 for Horizon: the mod API wiring of the camera smoothing.
//!
//! The smoothing state machine (the original's two Camera.update hooks run
//! as one) lives in `runtime.rs`, untouched: mode-change head logic, the
//! springs, the transition, the front-view fall-flying snap. The hooks
//! gather the pose facts and the settings answer, and the camera pose hook
//! applies the eased position. The eased turn waits on a camera-rotation
//! write in the API (README seams).
use mod_api::{
    self, CameraPoseEvent, ClientHost, Command, CommandEvent, Flow, RegistrationContext,
    RegisterClientCommandsEvent, Registrar, SettingSpec, SettingValue,
};

mod runtime;

use runtime::{Config, Mode, Pose, Quat};

#[derive(Default)]
struct Mod;

/// The mod's id: its settings' namespace and its command.
const MOD_ID: &str = "smooth_f5";

#[mod_api::horizon_mod]
impl mod_api::Hooks for Mod {
    /// Register the original's config fields as settings, same defaults.
    fn register(&self, registrar: &mut dyn Registrar, _context: &RegistrationContext) {
        let _ = registrar.register_setting(SettingSpec::choice(
            "mode",
            "setting.smooth_f5.mode",
            [
                "setting.smooth_f5.mode.transition",
                "setting.smooth_f5.mode.movement",
                "setting.smooth_f5.mode.always",
                "setting.smooth_f5.mode.never",
            ],
            2,
        ));
        let _ = registrar.register_setting(SettingSpec::toggle("pos", "setting.smooth_f5.pos", true));
        let _ = registrar.register_setting(SettingSpec::toggle("rot", "setting.smooth_f5.rot", true));
        let _ = registrar.register_setting(SettingSpec::range(
            "pos_stiffness",
            "setting.smooth_f5.pos_stiffness",
            0.01,
            1.0,
            0.01,
            0.3,
        ));
        let _ = registrar.register_setting(SettingSpec::range(
            "rot_stiffness",
            "setting.smooth_f5.rot_stiffness",
            0.01,
            1.0,
            0.01,
            0.7,
        ));
        let _ = registrar.register_setting(SettingSpec::range(
            "return_duration",
            "setting.smooth_f5.return_duration",
            1.0,
            100.0,
            1.0,
            10.0,
        ));
    }

    /// Once a frame, after vanilla placed the camera: run the smoothing
    /// state machine and apply the eased position. The eased turn (yaw,
    /// pitch and the roll) waits on a camera-rotation write in the API.
    fn camera_pose(&self, host: &dyn ClientHost, event: &mut CameraPoseEvent) {
        let camera = host.camera_type();
        let pose = Pose {
            pos: *event.position,
            yaw: event.yaw,
            pitch: event.pitch,
            detached: !camera.is_first_person(),
            mirrored: camera.is_mirrored(),
            fall_flying: fall_flying(host),
        };
        let config = read_config(host);
        if config.mode == Mode::Never {
            return;
        }
        let dt = runtime::camera_seconds();
        let out = {
            let mut state = runtime::state().lock().expect("smooth_f5 state lock");
            runtime::camera_hook(&pose, dt, &config, &mut state)
        };
        // The offset rides the camera's right, up and back; the pose's own
        // rotation carries it into world space.
        if out.offset != [0.0_f32; 3] {
            let q = Quat::rotation_yxz(event.yaw, event.pitch, 0.0);
            let world = q.transform(out.offset);
            for i in 0..3 {
                event.position[i] += world[i] as f64;
            }
        }
    }

    /// `/smoothf5` and its `sf5` short form, answered locally and consumed.
    fn register_client_commands(&self, _host: &mut dyn ClientHost, event: &mut RegisterClientCommandsEvent) {
        event.commands.push(Command::new("smoothf5 <mode|pos|rot|stiffness|return|status>"));
        event.commands.push(Command::new("sf5 <mode|pos|rot|stiffness|return|status>"));
    }

    fn command(&self, host: &mut dyn ClientHost, event: &mut CommandEvent) -> Flow {
        let body = event
            .command
            .trim()
            .strip_prefix("/smoothf5")
            .or_else(|| event.command.trim().strip_prefix("/sf5"));
        let Some(body) = body else {
            return Flow::Continue;
        };
        let words: Vec<&str> = body.split_whitespace().collect();
        let head = words.first().copied().unwrap_or("status");
        let arg = words.get(1).copied();
        let reply: String = match head {
            "status" => runtime::summary(&read_config(host)),
            "mode" => match arg.and_then(Mode::parse) {
                Some(mode) => {
                    set(host, "mode", SettingValue::Choice(mode.index()));
                    format!("smooth f5 {}", runtime::summary(&read_config(host)))
                }
                None => "usage: /smoothf5 mode <transition|movement|always|never>".to_owned(),
            },
            which @ ("pos" | "rot") => match arg {
                Some("on") => {
                    set(host, which, SettingValue::Toggle(true));
                    format!("smooth f5 {}", runtime::summary(&read_config(host)))
                }
                Some("off") => {
                    set(host, which, SettingValue::Toggle(false));
                    format!("smooth f5 {}", runtime::summary(&read_config(host)))
                }
                _ => format!("usage: /smoothf5 {which} <on|off>"),
            },
            "stiffness" => match (arg, words.get(2).and_then(|w| w.parse::<f32>().ok())) {
                (Some("pos"), Some(value)) if (0.01..=1.0).contains(&value) => {
                    set(host, "pos_stiffness", SettingValue::Range(value as f64));
                    format!("smooth f5 {}", runtime::summary(&read_config(host)))
                }
                (Some("rot"), Some(value)) if (0.01..=1.0).contains(&value) => {
                    set(host, "rot_stiffness", SettingValue::Range(value as f64));
                    format!("smooth f5 {}", runtime::summary(&read_config(host)))
                }
                _ => "usage: /smoothf5 stiffness <pos|rot> <0.01 to 1>".to_owned(),
            },
            "return" => match arg.and_then(|w| w.parse::<i32>().ok()) {
                Some(value) if (1..=100).contains(&value) => {
                    set(host, "return_duration", SettingValue::Range(value as f64));
                    format!("smooth f5 {}", runtime::summary(&read_config(host)))
                }
                _ => "usage: /smoothf5 return <1 to 100>".to_owned(),
            },
            _ => "usage: /smoothf5 <mode|pos|rot|stiffness|return|status>".to_owned(),
        };
        let _ = host.chat_line(mod_api::Text::literal(reply));
        Flow::Cancel
    }
}

/// Write one setting back (the command's changes persist like the
/// original's config file did).
fn set(host: &mut dyn ClientHost, key: &str, value: SettingValue) {
    let text = match value {
        SettingValue::Toggle(true) => "true".to_string(),
        SettingValue::Toggle(false) => "false".to_string(),
        SettingValue::Choice(index) => index.to_string(),
        SettingValue::Range(value) => value.to_string(),
        SettingValue::Color(value) => value.to_string(),
        _ => return,
    };
    host.set_setting(MOD_ID, key, &text);
}

/// The settings read into the config shape, defaults filling any gap.
fn read_config(host: &dyn ClientHost) -> Config {
    let mut cfg = Config::default();
    if let Some(index) = host.setting_choice("mode") {
        if let Some(mode) = Mode::from_index(index as usize) {
            cfg.mode = mode;
        }
    }
    if let Some(value) = host.setting_bool("pos") {
        cfg.pos = value;
    }
    if let Some(value) = host.setting_bool("rot") {
        cfg.rot = value;
    }
    if let Some(value) = host.setting_f64("pos_stiffness") {
        cfg.pos_stiffness = value as f32;
    }
    if let Some(value) = host.setting_f64("rot_stiffness") {
        cfg.rot_stiffness = value as f32;
    }
    if let Some(value) = host.setting_f64("return_duration") {
        cfg.return_duration = value as i32;
    }
    cfg
}

/// Whether the camera's entity is fall flying (the elytra glide), the
/// front-view snap's trigger in the original.
fn fall_flying(host: &dyn ClientHost) -> bool {
    host.camera_entity()
        .and_then(|entity| host.living(entity))
        .and_then(|living| living.is_gliding(host))
        .unwrap_or(false)
}
