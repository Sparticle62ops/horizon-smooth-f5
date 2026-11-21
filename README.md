# smooth_f5 (Horizon native mod)

A 1:1 Rust port of [Countered's Smooth F5](https://modrinth.com/mod/countereds-smooth-f5)
(MIT): the camera glides between perspectives instead of cutting, with the
original's mode (transition, movement, always, never), the position and
rotation springs and the return duration. The package is the `.hmod` source:
a native mod built against the pinned Horizon SDK (`api 0.2`, game 26.3).

## Behavior (the original's, kept)

- The smoothing state machine is the original's two Camera.update hooks run
  as one: the mode-change head logic, the springs, the transition and the
  front-view fall-flying snap, then the pose delta.
- Modes: transition (only the F5 cut smooths), movement (third person stays
  springy), always, never.
- The settings are the original's config fields, held on Horizon's
  Options > Mods page, and `/smoothf5` (short form `/sf5`) keeps the
  original's command surface: mode, pos, rot, stiffness pos|rot, return
  and status.

## Wiring

- camera_pose (client): the state machine runs once a frame after vanilla
  places the camera and the eased position offset rides the camera's right,
  up and back into the pose the game draws from.
- register: the original's config fields registered as settings, same
  defaults, labels in `assets/smooth_f5/lang/`.
- command + register_client_commands: `/smoothf5`, answered locally,
  written back through the settings, shown as a chat line, and consumed.
- The frame delta the springs need is measured between hook calls.

## Horizon seams

- The camera pose event takes position, yaw and pitch writes on the pinned
  API, so the eased pose applies in full: the rotation writes as the
  smoothed camera's yaw and pitch, and the position rides a world-space
  delta built on the smoothed camera's own frame (the same basis the
  runtime projects the spring offsets on).

## License

Upstream Countered's Smooth F5: MIT. This port carries the same license;
see LICENSE.
