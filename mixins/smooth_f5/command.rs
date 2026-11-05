// `/smoothf5` configures the mod: the smoothing mode, the position and
// rotation toggles, the spring stiffnesses and the first-person return
// duration. `/sf5` is the short alias. The parsing lives in the runtime
// module (`command_entry`); this helper is the hook's single entry point.
fn smooth_f5_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    crate::game::smooth_f5::command_entry(game, command, ran)
}
