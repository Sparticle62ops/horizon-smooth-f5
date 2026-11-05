// The camera-shake hook: every frame the game asks what camera shake to
// apply, and this module answers with the Countered's Smooth F5 smoothed-
// camera delta instead of nothing. The camera pose and player flags come
// off the game state here; the smoothing math lives in the runtime module.
fn smooth_f5_camera(
    game: &crate::game::Game<'_>,
    shake: &mut crate::game::native_hooks::NativeShake,
) {
    if !game.native_on(crate::game::smooth_f5::ID) {
        return;
    }
    let config = crate::game::smooth_f5::camera_config();
    if config.mode == crate::game::smooth_f5::Mode::Never {
        return;
    }
    let pose = crate::game::smooth_f5::Pose {
        pos: game.view.position,
        yaw: game.view.y_rot,
        pitch: game.view.x_rot,
        detached: !game.camera_type.first_person(),
        mirrored: game.camera_type == crate::game::perspective::CameraType::ThirdPersonFront,
        fall_flying: game.player.as_ref().is_some_and(|p| p.fall_flying),
    };
    let seconds = crate::game::smooth_f5::camera_seconds();
    let out = crate::game::smooth_f5::camera_hook(
        &pose,
        seconds,
        &config,
        &mut crate::game::smooth_f5::state().lock().expect("smooth_f5 state lock"),
    );
    if out.is_none() {
        return;
    }
    shake.yaw = out.yaw;
    shake.pitch = out.pitch;
    shake.roll = out.roll;
    shake.offset = out.offset;
}
