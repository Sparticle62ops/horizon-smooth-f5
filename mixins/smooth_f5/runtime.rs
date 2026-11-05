
/// The mod id, as `cargo horizon-mod check` and the mod screen see it.
pub const ID: &str = "smooth_f5";

/// The largest turn the camera-shake surface gives, in degrees, on each axis
/// (`SHAKE_MAX_DEGREES` in the game's `native_hooks`).
pub const SHAKE_MAX_DEGREES: f32 = 10.0;
/// The largest offset the camera-shake surface gives, in blocks, along each
/// axis (`SHAKE_MAX_OFFSET` in the game's `native_hooks`).
pub const SHAKE_MAX_OFFSET: f32 = 0.5;

const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;
const RAD_TO_DEG: f32 = 180.0 / std::f32::consts::PI;
const PI: f32 = std::f32::consts::PI;

/// The smoothing mode, as `SmoothingMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Transition,
    Movement,
    Always,
    Never,
}

impl Mode {
    pub fn parse(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "transition" => Some(Self::Transition),
            "movement" => Some(Self::Movement),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Transition => "transition",
            Self::Movement => "movement",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

/// The mod's config, as the original's `ConfigPlatformImpl` (the same keys and
/// defaults, in a small JSON file at `<config>/smooth-f5/config.json`).
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub mode: Mode,
    pub pos: bool,
    pub rot: bool,
    pub pos_stiffness: f32,
    pub rot_stiffness: f32,
    pub return_duration: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Always,
            pos: true,
            rot: true,
            pos_stiffness: 0.3,
            rot_stiffness: 0.7,
            return_duration: 10,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Option<Self> {
        let field = |key: &str| -> Option<String> {
            let marker = format!("\"{key}\"");
            let at = text.find(&marker)? + marker.len();
            let rest = text[at..].trim_start();
            let rest = rest.strip_prefix(':')?.trim_start();
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            Some(rest[..end].trim().to_owned())
        };
        let mut config = Config::default();
        if let Some(word) = field("mode") {
            config.mode = Mode::parse(word.trim_matches('"'))?;
        }
        if let Some(word) = field("pos") {
            config.pos = word == "true";
        }
        if let Some(word) = field("rot") {
            config.rot = word == "true";
        }
        if let Some(word) = field("posStiffness") {
            config.pos_stiffness = word.parse::<f32>().ok()?.clamp(0.01, 1.0);
        }
        if let Some(word) = field("rotStiffness") {
            config.rot_stiffness = word.parse::<f32>().ok()?.clamp(0.01, 1.0);
        }
        if let Some(word) = field("returnDuration") {
            config.return_duration = word.parse::<i32>().ok()?.clamp(1, 100);
        }
        Some(config)
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"mode\":\"{}\",\"pos\":{},\"rot\":{},\"posStiffness\":{},\"rotStiffness\":{},\"returnDuration\":{}}}",
            self.mode.name(),
            self.pos,
            self.rot,
            self.pos_stiffness,
            self.rot_stiffness,
            self.return_duration
        )
    }
}

/// The turn and offset the camera hook writes into the frame's camera shake:
/// turns in degrees and the offset in blocks along the camera's right, up and
/// back, exactly what `NativeShake` carries.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShakeOut {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub offset: [f32; 3],
}

impl ShakeOut {
    pub fn is_none(&self) -> bool {
        self.yaw == 0.0 && self.pitch == 0.0 && self.roll == 0.0 && self.offset == [0.0; 3]
    }
}

/// The smoothing state, the original's `CameraMixin` fields.
pub struct State {
    smooth_pos: [f64; 3],
    smooth_vel: [f64; 3],
    smooth_yaw: f32,
    smooth_pitch: f32,
    yaw_vel: f32,
    pitch_vel: f32,
    was_detached: bool,
    was_mirrored: bool,
    initialized: bool,
    should_snap_next: bool,
    trans_start_pos: [f64; 3],
    trans_start_yaw: f32,
    trans_start_pitch: f32,
    trans_target_yaw: f32,
    trans_prev_raw_yaw: f32,
    trans_delta_ready: bool,
    is_transitioning: bool,
    transition_progress: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            smooth_pos: [0.0; 3],
            smooth_vel: [0.0; 3],
            smooth_yaw: 0.0,
            smooth_pitch: 0.0,
            yaw_vel: 0.0,
            pitch_vel: 0.0,
            was_detached: false,
            was_mirrored: false,
            initialized: false,
            should_snap_next: false,
            trans_start_pos: [0.0; 3],
            trans_start_yaw: 0.0,
            trans_start_pitch: 0.0,
            trans_target_yaw: 0.0,
            trans_prev_raw_yaw: 0.0,
            trans_delta_ready: false,
            is_transitioning: false,
            transition_progress: 0.0,
        }
    }
}

impl State {
    fn snap_to(&mut self, pos: [f64; 3], yaw: f32, pitch: f32) {
        self.smooth_pos = pos;
        self.smooth_vel = [0.0; 3];
        self.smooth_yaw = yaw;
        self.smooth_pitch = pitch;
        self.yaw_vel = 0.0;
        self.pitch_vel = 0.0;
    }

    fn reset_states(&mut self, pos: [f64; 3], yaw: f32, pitch: f32) {
        self.smooth_pos = pos;
        self.smooth_yaw = yaw;
        self.smooth_pitch = pitch;
        self.smooth_vel = [0.0; 3];
        self.yaw_vel = 0.0;
        self.pitch_vel = 0.0;
    }

    /// The original's `stepPos`: a spring on each axis, substeps of 0.25.
    fn step_pos(&mut self, target: [f64; 3], dt: f32, stiffness: f32) {
        let damping = 2.0 * stiffness.sqrt();
        let mut left = dt;
        while left > 0.0 {
            let step = left.min(0.25) as f64;
            for axis in 0..3 {
                let diff = target[axis] - self.smooth_pos[axis];
                let acceleration = diff * f64::from(stiffness)
                    - self.smooth_vel[axis] * f64::from(damping);
                self.smooth_vel[axis] += acceleration * step;
                self.smooth_pos[axis] += self.smooth_vel[axis] * step;
            }
            left -= 0.25;
        }
    }

    /// The original's `stepRot`: springs on yaw and pitch, substeps of 0.1.
    fn step_rot(&mut self, target_yaw: f32, target_pitch: f32, dt: f32, stiffness: f32) {
        let damping = 2.0 * stiffness.sqrt();
        let mut left = dt;
        while left > 0.0 {
            let step = left.min(0.1);
            let yaw_diff = wrap_degrees(target_yaw - self.smooth_yaw);
            let pitch_diff = target_pitch - self.smooth_pitch;
            let yaw_acc = yaw_diff * stiffness - self.yaw_vel * damping;
            let pitch_acc = pitch_diff * stiffness - self.pitch_vel * damping;
            self.yaw_vel += yaw_acc * step;
            self.pitch_vel += pitch_acc * step;
            self.smooth_yaw += self.yaw_vel * step;
            self.smooth_pitch += self.pitch_vel * step;
            left -= 0.1;
        }
        self.smooth_yaw = wrap_degrees(self.smooth_yaw);
    }

    /// The original's `applyTransitionLogic`: the eased return to first person.
    fn apply_transition_logic(
        &mut self,
        target_pos: [f64; 3],
        target_yaw: f32,
        target_pitch: f32,
        dt: f32,
        duration: i32,
    ) {
        if !self.trans_delta_ready {
            self.trans_target_yaw =
                self.trans_start_yaw + wrap_degrees(target_yaw - self.trans_start_yaw);
            self.trans_prev_raw_yaw = target_yaw;
            self.trans_delta_ready = true;
        } else {
            let frame_delta = wrap_degrees(target_yaw - self.trans_prev_raw_yaw);
            self.trans_target_yaw += frame_delta;
            self.trans_prev_raw_yaw = target_yaw;
        }
        let duration = duration.max(1) as f32;
        self.transition_progress = (self.transition_progress + dt / duration).min(1.0);
        let t = self.transition_progress;
        let ease = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
        for axis in 0..3 {
            self.smooth_pos[axis] =
                self.trans_start_pos[axis] + (target_pos[axis] - self.trans_start_pos[axis]) * f64::from(ease);
        }
        self.smooth_yaw = wrap_degrees(
            self.trans_start_yaw + (self.trans_target_yaw - self.trans_start_yaw) * ease,
        );
        self.smooth_pitch =
            self.trans_start_pitch + (target_pitch - self.trans_start_pitch) * ease;
        if self.transition_progress >= 1.0 {
            self.is_transitioning = false;
            self.snap_to(target_pos, target_yaw, target_pitch);
        }
    }
}

/// `Mth.wrapDegrees`.
pub fn wrap_degrees(value: f32) -> f32 {
    let wrapped = value % 360.0;
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else if wrapped < -180.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

/// The camera pose the hook sees, all plain numbers so the module stays free
/// of the game's types.
pub struct Pose {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub detached: bool,
    pub mirrored: bool,
    pub fall_flying: bool,
}

/// The frame's smoothed-camera answer: turn in degrees and the offset in
/// blocks along the camera's right, up and back.
///
/// This is the whole port of the original's two `Camera.update` hooks run as
/// one: the mode-change head logic, the springs, the transition and the
/// front-view fall-flying snap, then the pose delta the shake surface applies.
pub fn camera_hook(
    pose: &Pose,
    dt: Option<f32>,
    config: &Config,
    state: &mut State,
) -> ShakeOut {
    if config.mode == Mode::Never {
        return ShakeOut::default();
    }

    // The original reads the frame delta from the game's tracker; this port
    // measures it and keeps it in ticks, the unit the springs are tuned in.
    let dt = dt
        .map(|seconds| (seconds.max(0.0) * 20.0).clamp(0.001, 1.0))
        .unwrap_or(1.0 / 20.0);

    // The original's head hook: the camera mode changed.
    if state.was_detached != pose.detached || state.was_mirrored != pose.mirrored {
        if !state.initialized {
            state.snap_to(pose.pos, pose.yaw, pose.pitch);
            state.initialized = true;
        }
        if config.mode == Mode::Transition
            || (config.mode == Mode::Always && !pose.detached && state.was_detached)
        {
            state.trans_start_pos = state.smooth_pos;
            state.trans_start_yaw = state.smooth_yaw;
            state.trans_start_pitch = state.smooth_pitch;
            state.trans_delta_ready = false;
            state.is_transitioning = true;
            state.transition_progress = 0.0;
        } else {
            state.is_transitioning = false;
            if config.mode == Mode::Movement {
                state.should_snap_next = true;
            }
        }
    }
    state.was_detached = pose.detached;
    state.was_mirrored = pose.mirrored;

    // The original's tail hook, after the camera aligned with the entity.
    if !pose.detached && !state.is_transitioning {
        state.reset_states(pose.pos, pose.yaw, pose.pitch);
        return ShakeOut::default();
    }

    let should_smooth = match config.mode {
        Mode::Always => true,
        Mode::Transition => state.is_transitioning,
        Mode::Movement => pose.detached && !state.is_transitioning,
        Mode::Never => false,
    };

    if should_smooth {
        if state.is_transitioning {
            state.apply_transition_logic(pose.pos, pose.yaw, pose.pitch, dt, config.return_duration);
        } else if state.should_snap_next {
            state.snap_to(pose.pos, pose.yaw, pose.pitch);
            state.should_snap_next = false;
        } else {
            state.step_pos(pose.pos, dt, config.pos_stiffness);
            state.step_rot(pose.yaw, pose.pitch, dt, config.rot_stiffness);
        }
    } else {
        state.snap_to(pose.pos, pose.yaw, pose.pitch);
        if !pose.detached {
            state.is_transitioning = false;
        }
    }

    if pose.mirrored && pose.fall_flying {
        state.reset_states(pose.pos, pose.yaw, pose.pitch);
    }

    // The original's `apply` writes the smoothed pose straight onto the
    // camera; this port answers with the delta the camera-shake surface
    // applies instead, and keeps the state in step with what the surface can
    // actually render (its per-frame bounds).
    apply_delta(pose, config, state)
}

/// The smoothed-vs-vanilla delta, clamped to the shake surface's bounds and
/// synced back into the state.
fn apply_delta(pose: &Pose, config: &Config, state: &mut State) -> ShakeOut {
    let mut out = ShakeOut::default();

    if config.rot {
        let yaw_diff = wrap_degrees(state.smooth_yaw - pose.yaw).clamp(-SHAKE_MAX_DEGREES, SHAKE_MAX_DEGREES);
        let pitch_diff = (state.smooth_pitch - pose.pitch).clamp(-SHAKE_MAX_DEGREES, SHAKE_MAX_DEGREES);
        let rendered_yaw = wrap_degrees(pose.yaw + yaw_diff);
        let rendered_pitch = pose.pitch + pitch_diff;

        // The smoothed camera's orientation, built the way `Camera.setRotation`
        // builds the vanilla one.
        let q_v = Quat::rotation_yxz(PI - pose.yaw * DEG_TO_RAD, -pose.pitch * DEG_TO_RAD, 0.0);
        let q_s = Quat::rotation_yxz(
            PI - rendered_yaw * DEG_TO_RAD,
            -rendered_pitch * DEG_TO_RAD,
            0.0,
        );
        // The view-space rotation between the vanilla and smoothed cameras:
        // the smoothed orientation's inverse, then the vanilla's.
        let delta = q_s.conjugate().multiply(q_v);
        let matrix = delta.matrix();
        // `Rz(roll) * Rx(pitch) * Ry(yaw)`, the order the shake surface applies.
        let (yaw_sh, pitch_sh, roll_sh) = if matrix[2][1].abs() < 0.999 {
            (
                f32::atan2(-matrix[2][0], matrix[2][2]),
                f32::asin(matrix[2][1].clamp(-1.0, 1.0)),
                f32::atan2(-matrix[0][1], matrix[1][1]),
            )
        } else {
            (f32::atan2(matrix[0][2], matrix[0][0]), matrix[2][1].signum() * PI / 2.0, 0.0)
        };
        out.yaw = yaw_sh * RAD_TO_DEG;
        out.pitch = pitch_sh * RAD_TO_DEG;
        out.roll = roll_sh * RAD_TO_DEG;
        state.smooth_yaw = rendered_yaw;
        state.smooth_pitch = rendered_pitch;
    } else {
        state.smooth_yaw = pose.yaw;
        state.smooth_pitch = pose.pitch;
        state.yaw_vel = 0.0;
        state.pitch_vel = 0.0;
    }

    if config.pos {
        // The offset along the smoothed camera's right, up and back.
        let q_s = Quat::rotation_yxz(
            PI - state.smooth_yaw * DEG_TO_RAD,
            -state.smooth_pitch * DEG_TO_RAD,
            0.0,
        );
        let right = q_s.transform([1.0, 0.0, 0.0]);
        let up = q_s.transform([0.0, 1.0, 0.0]);
        let back = q_s.transform([0.0, 0.0, 1.0]);
        let d = [
            state.smooth_pos[0] - pose.pos[0],
            state.smooth_pos[1] - pose.pos[1],
            state.smooth_pos[2] - pose.pos[2],
        ];
        let mut offset = [
            dot(right, d).clamp(-SHAKE_MAX_OFFSET, SHAKE_MAX_OFFSET),
            dot(up, d).clamp(-SHAKE_MAX_OFFSET, SHAKE_MAX_OFFSET),
            dot(back, d).clamp(-SHAKE_MAX_OFFSET, SHAKE_MAX_OFFSET),
        ];
        for value in &mut offset {
            if !value.is_finite() {
                *value = 0.0;
            }
        }
        out.offset = offset;
        // The state follows what the surface renders: the vanilla position
        // moved by the clamped offset.
        for axis in 0..3 {
            let axis_vector = [right[axis], up[axis], back[axis]];
            let world_delta: f64 = offset
                .iter()
                .zip(axis_vector)
                .map(|(amount, unit)| f64::from(*amount) * f64::from(unit))
                .sum();
            state.smooth_pos[axis] = pose.pos[axis] + world_delta;
        }
    } else {
        // The original only skips `setPosition`; the position state follows
        // the vanilla camera so the springs stay honest for later.
        state.smooth_pos = pose.pos;
        state.smooth_vel = [0.0; 3];
    }

    out
}

fn dot(a: [f32; 3], b: [f64; 3]) -> f32 {
    (f64::from(a[0]) * b[0] + f64::from(a[1]) * b[1] + f64::from(a[2]) * b[2]) as f32
}

/// A JOML `Quaternionf` (x, y, z, w), the same values the game's camera uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    pub fn rotation_yxz(angle_y: f32, angle_x: f32, angle_z: f32) -> Self {
        let sx = (angle_x * 0.5).sin();
        let cx = cos_from_sin(sx, angle_x * 0.5);
        let sy = (angle_y * 0.5).sin();
        let cy = cos_from_sin(sy, angle_y * 0.5);
        let sz = (angle_z * 0.5).sin();
        let cz = cos_from_sin(sz, angle_z * 0.5);
        let x = cy * sx;
        let y = sy * cx;
        let z = sy * sx;
        let w = cy * cx;
        Self {
            x: x * cz + y * sz,
            y: y * cz - x * sz,
            z: w * sz - z * cz,
            w: w * cz + z * sz,
        }
    }

    pub fn conjugate(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    /// The Hamilton product, so `a.multiply(b)` is the matrix product
    /// `M(a) * M(b)`.
    pub fn multiply(self, other: Self) -> Self {
        let (ax, ay, az, aw) = (self.x, self.y, self.z, self.w);
        let (bx, by, bz, bw) = (other.x, other.y, other.z, other.w);
        Self {
            x: aw * bx + ax * bw + ay * bz - az * by,
            y: aw * by - ax * bz + ay * bw + az * bx,
            z: aw * bz + ax * by - ay * bx + az * bw,
            w: aw * bw - ax * bx - ay * by - az * bz,
        }
    }

    /// `Quaternionf.transform(x, y, z)` (`Vector3f.rotate`).
    pub fn transform(self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = v;
        let xx = self.x * self.x;
        let yy = self.y * self.y;
        let zz = self.z * self.z;
        let ww = self.w * self.w;
        let xy = self.x * self.y;
        let xz = self.x * self.z;
        let yz = self.y * self.z;
        let xw = self.x * self.w;
        let zw = self.z * self.w;
        let yw = self.y * self.w;
        let k = 1.0 / (xx + yy + zz + ww);
        [
            (xx - yy - zz + ww) * k * x + 2.0 * (xy - zw) * k * y + 2.0 * (xz + yw) * k * z,
            2.0 * (xy + zw) * k * x + (yy - xx - zz + ww) * k * y + 2.0 * (yz - xw) * k * z,
            2.0 * (xz - yw) * k * x + 2.0 * (yz + xw) * k * y + (zz - xx - yy + ww) * k * z,
        ]
    }

    /// The rotation as a row-major matrix (`m[row][col]`), the matrix that
    /// `transform` applies.
    pub fn matrix(self) -> [[f32; 3]; 3] {
        let ex = self.transform([1.0, 0.0, 0.0]);
        let ey = self.transform([0.0, 1.0, 0.0]);
        let ez = self.transform([0.0, 0.0, 1.0]);
        [[ex[0], ey[0], ez[0]], [ex[1], ey[1], ez[1]], [ex[2], ey[2], ez[2]]]
    }
}

/// The game's own `cos_from_sin` (`camera.rs`), so this module's quaternions
/// match the camera's exactly.
fn cos_from_sin(sin: f32, angle: f32) -> f32 {
    let cos = (1.0 - sin * sin).sqrt();
    let a = angle + std::f32::consts::FRAC_PI_2;
    let mut b = a - (a / std::f32::consts::TAU) as i32 as f32 * std::f32::consts::TAU;
    if f64::from(b) < 0.0 {
        b += std::f32::consts::TAU;
    }
    if b >= PI {
        -cos
    } else {
        cos
    }
}


/// Reads the config file at `<config_dir>/smooth_f5/config.json`, falling
/// back to the defaults on anything unreadable.
pub fn load_config(config_dir: &std::path::Path) -> Config {
    std::fs::read_to_string(config_dir.join("smooth_f5").join("config.json"))
        .ok()
        .and_then(|text| Config::parse(&text))
        .unwrap_or_default()
}

/// Writes the config file, creating the folder when needed.
pub fn save_config(config_dir: &std::path::Path, config: &Config) -> std::io::Result<()> {
    let dir = config_dir.join("smooth_f5");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("config.json"), config.to_json())
}

/// The shared smoothing state the camera hook drives.
pub fn state() -> &'static std::sync::Mutex<State> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<State>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(State::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn close3(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| close(*x, *y))
    }

    #[test]
    fn wrap_degrees_wraps_into_the_half_open_range() {
        assert!(close(wrap_degrees(190.0), -170.0));
        assert!(close(wrap_degrees(-190.0), 170.0));
        assert!(close(wrap_degrees(360.0), 0.0));
        assert!(close(wrap_degrees(45.0), 45.0));
    }

    #[test]
    fn camera_faces_south_at_zero_yaw_like_the_games_camera() {
        let q = Quat::rotation_yxz(PI - 0.0, 0.0, 0.0);
        assert!(close3(q.transform([0.0, 0.0, -1.0]), [0.0, 0.0, 1.0]));
        let q = Quat::rotation_yxz(PI - 90.0 * DEG_TO_RAD, 0.0, 0.0);
        assert!(close3(q.transform([0.0, 0.0, -1.0]), [-1.0, 0.0, 0.0]));
    }

    #[test]
    fn view_rotation_sends_the_world_forward_to_view_front() {
        // yaw 0 looks south (+z); yaw 90 looks west (-x); both land on the
        // view's front (-z). World to view is the quat matrix's transpose.
        let q = Quat::rotation_yxz(PI, 0.0, 0.0);
        assert!(close3(mul(transpose(q.matrix()), [0.0, 0.0, 1.0]), [0.0, 0.0, -1.0]));
        let q = Quat::rotation_yxz(PI - std::f32::consts::FRAC_PI_2, 0.0, 0.0);
        assert!(close3(mul(transpose(q.matrix()), [-1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]));
    }

    fn transpose(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
        [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]]
    }

    fn mulm(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let mut out = [[0.0f32; 3]; 3];
        for row in 0..3 {
            for col in 0..3 {
                out[row][col] = a[row][0] * b[0][col] + a[row][1] * b[1][col] + a[row][2] * b[2][col];
            }
        }
        out
    }

    fn rot_y(angle: f32) -> [[f32; 3]; 3] {
        let (s, c) = angle.sin_cos();
        [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
    }

    fn rot_x(angle: f32) -> [[f32; 3]; 3] {
        let (s, c) = angle.sin_cos();
        [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
    }

    fn rot_z(angle: f32) -> [[f32; 3]; 3] {
        let (s, c) = angle.sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    }

    fn mul(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    #[test]
    fn pose_delta_matches_the_smoothed_rotation_exactly() {
        // The rendered rotation must be the smoothed one: shake applied to the
        // vanilla view lands on the smoothed camera, not near it.
        for &(yv, pv, ys, ps) in &[
            (0.0f32, 0.0f32, 5.0f32, 0.0f32),
            (170.0, 0.0, -170.0, -8.0),
            (10.0, -30.0, 12.0, -28.0),
            (-40.0, 80.0, -35.0, 89.0),
            (0.0, -89.0, 0.0, 88.0),
        ] {
            let q_v = Quat::rotation_yxz(PI - yv * DEG_TO_RAD, -pv * DEG_TO_RAD, 0.0);
            let q_s = Quat::rotation_yxz(PI - ys * DEG_TO_RAD, -ps * DEG_TO_RAD, 0.0);
            let delta = q_s.conjugate().multiply(q_v);
            let m = delta.matrix();
            let (yaw, pitch, roll) = if m[2][1].abs() < 0.999 {
                (
                    f32::atan2(-m[2][0], m[2][2]),
                    f32::asin(m[2][1].clamp(-1.0, 1.0)),
                    f32::atan2(-m[0][1], m[1][1]),
                )
            } else {
                (f32::atan2(m[0][2], m[0][0]), m[2][1].signum() * PI / 2.0, 0.0)
            };
            // Rebuild the shake the game composes: Rz(roll) Rx(pitch) Ry(yaw)
            // applied to the vanilla view must land exactly on the smoothed
            // view. World to view is the quat matrix's transpose.
            let rebuilt = mulm(rot_z(roll), mulm(rot_x(pitch), rot_y(yaw)));
            let total = mulm(rebuilt, transpose(q_v.matrix()));
            let want = transpose(q_s.matrix());
            for row in 0..3 {
                assert!(
                    close3(total[row], want[row]),
                    "row {row} for ({yv}, {pv}) -> ({ys}, {ps}): got {:?} want {:?}",
                    total[row], want[row]
                );
            }
        }
    }

    #[test]
    fn offset_sits_along_the_smoothed_camera_axes() {
        // A camera looking south has its view back pointing north, so a
        // smoothed camera 0.3 blocks south of the vanilla one answers with
        // offset -0.3 along back and nothing else.
        let config = Config::default();
        let mut state = State::default();
        let pose = Pose {
            pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            detached: true,
            mirrored: false,
            fall_flying: false,
        };
        state.snap_to([0.0, 64.0, 0.3], 0.0, 0.0);
        state.initialized = true;
        state.was_detached = true;
        let out = apply_delta(&pose, &config, &mut state);
        assert!(close(out.offset[2], -0.3), "back {:?}", out.offset);
        assert!(close(out.offset[0], 0.0) && close(out.offset[1], 0.0));
        // The rendered position stays 0.3 blocks south: the clamped sync
        // rounds the offset back onto the same spot.
        assert!((state.smooth_pos[2] - 0.3).abs() < 1e-4);
        // A bigger gap clamps to the surface's half-block ceiling per axis.
        state.snap_to([0.0, 64.0, 3.0], 0.0, 0.0);
        let out = apply_delta(&pose, &config, &mut state);
        assert!(close(out.offset[2], -SHAKE_MAX_OFFSET), "back {:?}", out.offset);
    }

    #[test]
    fn spring_pulls_the_camera_onto_the_target() {
        let mut state = State::default();
        state.snap_to([0.0; 3], 0.0, 0.0);
        let target = [3.0, 5.0, -2.0];
        for _ in 0..200 {
            state.step_pos(target, 1.0, 0.3);
        }
        for axis in 0..3 {
            assert!((state.smooth_pos[axis] - target[axis]).abs() < 0.01);
        }
        for _ in 0..200 {
            state.step_rot(170.0, -20.0, 1.0, 0.7);
        }
        assert!((wrap_degrees(state.smooth_yaw - 170.0)).abs() < 0.1);
        assert!((state.smooth_pitch + 20.0).abs() < 0.1);
    }

    #[test]
    fn movement_mode_snaps_once_after_a_camera_change() {
        let config = Config { mode: Mode::Movement, ..Config::default() };
        let mut state = State::default();
        let pose = |detached| Pose {
            pos: [1.0, 2.0, 3.0],
            yaw: 10.0,
            pitch: 0.0,
            detached,
            mirrored: false,
            fall_flying: false,
        };
        // First person: no smoothing, states reset.
        let out = camera_hook(&pose(false), Some(0.05), &config, &mut state);
        assert!(out.is_none());
        // Switch to third person: the movement mode snaps once instead of
        // easing, so the frame's delta is none.
        let out = camera_hook(&pose(true), Some(0.05), &config, &mut state);
        assert!(out.is_none());
        assert!(!state.should_snap_next);
        // From then on the spring runs and answers with a pose.
        let mut state2 = State::default();
        let _ = camera_hook(&pose(false), Some(0.05), &config, &mut state2);
        let _ = camera_hook(&pose(true), Some(0.05), &config, &mut state2);
        let moved = Pose { pos: [1.0, 2.0, 5.0], ..pose(true) };
        let out = camera_hook(&moved, Some(0.05), &config, &mut state2);
        assert!(!out.is_none());
    }

    #[test]
    fn transition_eases_back_into_first_person() {
        let config = Config { mode: Mode::Transition, ..Config::default() };
        let mut state = State::default();
        let third = Pose {
            pos: [0.0, 64.0, 4.0],
            yaw: 0.0,
            pitch: 0.0,
            detached: true,
            mirrored: false,
            fall_flying: false,
        };
        let first = Pose { pos: [0.0, 64.0, 0.0], detached: false, ..third };
        let _ = camera_hook(&third, Some(0.05), &config, &mut state);
        // The switch to first person starts the transition; each frame's
        // offset shrinks toward nothing.
        let mut last_offset = f32::INFINITY;
        let mut frames = 0;
        while state.is_transitioning && frames < 400 {
            let out = camera_hook(&first, Some(0.05), &config, &mut state);
            let size = out.offset[2].abs();
            assert!(size <= last_offset + 1e-6, "offset grew mid-transition");
            last_offset = size;
            frames += 1;
        }
        assert!(!state.is_transitioning, "the transition never finished");
        assert!((state.smooth_pos[2] - 0.0).abs() < 0.01);
        assert!(last_offset < 0.5);
    }

    #[test]
    fn never_mode_is_inert() {
        let config = Config { mode: Mode::Never, ..Config::default() };
        let mut state = State::default();
        let pose = Pose {
            pos: [1.0, 2.0, 3.0],
            yaw: 0.0,
            pitch: 0.0,
            detached: true,
            mirrored: false,
            fall_flying: false,
        };
        assert!(camera_hook(&pose, Some(0.05), &config, &mut state).is_none());
    }

    #[test]
    fn big_deltas_clamp_to_the_shake_bounds_and_the_state_follows() {
        let config = Config::default();
        let mut state = State::default();
        let pose = Pose {
            pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            detached: true,
            mirrored: false,
            fall_flying: false,
        };
        state.snap_to([0.0, 64.0, 0.0], 0.0, 0.0);
        state.initialized = true;
        state.was_detached = true;
        state.smooth_yaw = 90.0;
        let out = apply_delta(&pose, &config, &mut state);
        assert!(out.yaw.abs() <= SHAKE_MAX_DEGREES + 1e-4);
        assert!(close(state.smooth_yaw, SHAKE_MAX_DEGREES));
    }

    #[test]
    fn front_view_fall_flying_resets() {
        let config = Config::default();
        let mut state = State::default();
        let pose = Pose {
            pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            detached: true,
            mirrored: true,
            fall_flying: true,
        };
        state.smooth_yaw = 40.0;
        state.smooth_pos = [3.0, 70.0, 3.0];
        state.was_detached = true;
        let out = camera_hook(&pose, Some(0.05), &config, &mut state);
        assert!(out.is_none());
        assert!(close(state.smooth_yaw, 0.0));
        assert_eq!(state.smooth_pos, [0.0, 64.0, 0.0]);
    }

    #[test]
    fn config_round_trips() {
        let config = Config {
            mode: Mode::Movement,
            pos: false,
            rot: true,
            pos_stiffness: 0.55,
            rot_stiffness: 0.2,
            return_duration: 33,
        };
        let parsed = Config::parse(&config.to_json()).unwrap();
        assert_eq!(parsed.mode, Mode::Movement);
        assert!(!parsed.pos && parsed.rot);
        assert!(close(parsed.pos_stiffness, 0.55));
        assert!(close(parsed.rot_stiffness, 0.2));
        assert_eq!(parsed.return_duration, 33);
        assert!(Config::parse("{\"mode\":\"weird\"}").is_none());
        assert_eq!(Config::parse("{}").unwrap().mode, Mode::Always);
    }

    #[test]
    fn mode_names_parse_back() {
        for mode in [Mode::Transition, Mode::Movement, Mode::Always, Mode::Never] {
            assert_eq!(Mode::parse(mode.name()), Some(mode));
        }
    }
}

// Camera-hook glue, shared with the camera helper: the config cached from
// disk and the frame clock that supplies each hook call its delta time.
pub fn camera_config() -> Config {
    static CONFIG: std::sync::OnceLock<std::sync::Mutex<Option<Config>>> = std::sync::OnceLock::new();
    let cell = CONFIG.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = cell.lock().expect("smooth_f5 config lock");
    if let Some(config) = *guard {
        return config;
    }
    let loaded = crate::game::Game::native_config_dir()
        .ok()
        .map(|dir| load_config(&dir))
        .unwrap_or_else(Config::default);
    *guard = Some(loaded);
    loaded
}

pub fn camera_seconds() -> Option<f32> {
    static LAST: std::sync::OnceLock<std::sync::Mutex<Option<std::time::Instant>>> = std::sync::OnceLock::new();
    let cell = LAST.get_or_init(|| std::sync::Mutex::new(None));
    let mut last = cell.lock().expect("smooth_f5 frame lock");
    let now = std::time::Instant::now();
    let delta = last.map(|then| now.duration_since(then).as_secs_f32());
    *last = Some(now);
    delta
}

// Command glue, shared with the command helper.
fn config_slot() -> &'static std::sync::Mutex<Option<Config>> {
    static CONFIG: std::sync::OnceLock<std::sync::Mutex<Option<Config>>> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| std::sync::Mutex::new(None))
}

fn cached_config() -> Config {
    let mut guard = config_slot().lock().expect("smooth_f5 config lock");
    if guard.is_none() {
        *guard = Some(
            crate::game::Game::native_config_dir()
                .ok()
                .map(|dir| load_config(&dir))
                .unwrap_or_else(Config::default),
        );
    }
    guard.unwrap_or_else(Config::default)
}

fn store_config(game: &mut crate::game::Game<'_>, config: Config) {
    let dir = crate::game::Game::native_config_dir();
    let saved = dir.is_ok() && save_config(&dir.expect("checked"), &config).is_ok();
    *config_slot().lock().expect("smooth_f5 config lock") = Some(config);
    let word = if saved {
        "saved"
    } else {
        "set for this session (the config file could not be written)"
    };
    game.native_chat(&format!("smooth f5 {} {}", summary(&config), word));
}

fn summary(config: &Config) -> String {
    format!(
        "mode {}, pos {}, rot {}, stiffness {:.2}/{:.2}, return {}",
        config.mode.name(),
        on_off(config.pos),
        on_off(config.rot),
        config.pos_stiffness,
        config.rot_stiffness,
        config.return_duration
    )
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

/// Handles `/smoothf5` and `/sf5`: parses the subcommand, applies config
/// changes through the store and answers in chat, mirroring the original
/// Countered's Smooth F5 command surface.
pub fn command_entry(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !game.native_on(ID) {
        return;
    }
    let body = command
        .strip_prefix("/smoothf5")
        .or_else(|| command.strip_prefix("/sf5"));
    let mut words = match body {
        Some(rest) => rest.split_whitespace().map(|w| w.to_owned()).collect::<Vec<_>>(),
        None => return,
    };
    let mut config = cached_config();
    let head = if words.is_empty() { None } else { Some(words.remove(0)) };
    let reply = match head.as_deref() {
        None | Some("status") => summary(&config),
        Some("mode") => match words.first().and_then(|w| Mode::parse(w)) {
            Some(mode) => {
                config.mode = mode;
                store_config(game, config);
                return;
            }
            None => "usage: /smoothf5 mode <transition|movement|always|never>".to_owned(),
        },
        Some(which @ ("pos" | "rot")) => {
            let which_pos = which == "pos";
            match words.first().map(|w| w.as_str()) {
                Some("on") => {
                    if which_pos {
                        config.pos = true;
                    } else {
                        config.rot = true;
                    }
                    store_config(game, config);
                    return;
                }
                Some("off") => {
                    if which_pos {
                        config.pos = false;
                    } else {
                        config.rot = false;
                    }
                    store_config(game, config);
                    return;
                }
                _ => format!("usage: /smoothf5 {which} <on|off>"),
            }
        }
        Some("stiffness") => match (words.first().map(|w| w.as_str()), words.get(1).and_then(|w| w.parse::<f32>().ok())) {
            (Some("pos"), Some(value)) if (0.01..=1.0).contains(&value) => {
                config.pos_stiffness = value;
                store_config(game, config);
                return;
            }
            (Some("rot"), Some(value)) if (0.01..=1.0).contains(&value) => {
                config.rot_stiffness = value;
                store_config(game, config);
                return;
            }
            _ => "usage: /smoothf5 stiffness <pos|rot> <0.01 to 1>".to_owned(),
        },
        Some("return") => match words.first().and_then(|w| w.parse::<i32>().ok()) {
            Some(value) if (1..=100).contains(&value) => {
                config.return_duration = value;
                store_config(game, config);
                return;
            }
            _ => "usage: /smoothf5 return <1 to 100>".to_owned(),
        },
        _ => "usage: /smoothf5 <mode|pos|rot|stiffness|return|status>".to_owned(),
    };
    game.native_chat(&reply);
    *ran = true;
}
