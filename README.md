# smooth_f5 (Horizon port)

A Horizon port of [Countered's Smooth F5](https://github.com/Coun7ered/smooth_f5)
(CC0-1.0, by Countered), which smooths the third-person camera: no more hard
snaps when you toggle F5, gentle springs while you move, and an eased return
into first person.

## How it works in Horizon

The original hooks Minecraft's `Camera.update` and rewrites the camera pose
each frame. Horizon's camera is native, so this port rides the game's
camera-shake surface instead: every frame the vanilla camera aligns as usual,
then the mod answers the camera-shake hook with the turn (in degrees) and the
offset (in blocks, along the camera's right, up and back) that carries the
rendered camera onto the smoothed pose the original would have written.

Everything else is a faithful port of the original's `CameraMixin`:

- The same four smoothing modes (transition, movement, always, never) with the
  same defaults (always, both toggles on, stiffness 0.3/0.7, return 10).
- The same spring physics for position (substeps of 0.25) and rotation
  (substeps of 0.1), with damping twice the square root of the stiffness.
- The same eased cubic return into first person, re-anchored each frame so a
  moving target stays smooth.
- The same front-view fall-flying reset, so elytra flight stays crisp.

The camera-shake surface clamps each frame's turn to 10 degrees and each
offset axis to 0.5 blocks. The mod answers within those bounds and keeps its
state in step with what actually rendered, so a fast flick eases over a few
frames instead of jumping.

## Commands

- `/smoothf5 status` (or `/sf5`): show the current settings.
- `/smoothf5 mode <transition|movement|always|never>`
- `/smoothf5 pos <on|off>` and `/smoothf5 rot <on|off>`
- `/smoothf5 stiffness <pos|rot> <0.01 to 1>`
- `/smoothf5 return <1 to 100>`

Settings live at `<config>/smooth_f5/config.json`.

## License

CC0-1.0, like the original. See [LICENSE](LICENSE).
