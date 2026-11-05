# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The one camera rig the watch rig gets (spec section 3, During play): an own-fog camera
# with free-look and a follow-commander toggle.
#
# "Own-fog" is not something this script does. What the camera can show is what the
# gateway sent this seat's connection, and the gateway decides that (decisions-log item
# 107 (5)); the camera holds no visibility rule of its own and has no way to show more.
#
# Free-look: WASD or the arrow keys pan over the ground, Q and E lower and raise, the mouse
# wheel zooms on what is under the cursor, and the right mouse button held down turns the view.
# The lobby's Follow
# button (`lobby.gd`'s `_toggle_follow`) toggles following the commander; no key does. All
# of it is presentation - floats, easing and a camera - and none of it reaches the match.
#
# The keys are the camera's only while no GUI control has keyboard focus (decisions-log
# item 123 (2) 8): while the player types a note or a wizard value (or a file name in an
# embedded dialog), W, A, S, D, Q, E and the arrows are text, not camera moves. A mouse
# press on the 3D view that no control takes - a right-click, a wheel notch, or a
# left-click that `editor.gd` does not take as a beacon or ground click - releases that
# focus, so the keys come back without a restart. Godot 4.7 keeps a text field's focus
# through a click on empty space, which is why the rig releases it.
#
# THE WHEEL (S2's stage demo, decisions-log item 155 (3) 4). It used to zoom about the focus,
# the map's middle from the whole map's framing, by an eighth a notch, so a base in a corner
# slid off screen before it grew and the demo's base kit could not be told apart. Now each
# notch takes about a quarter off the distance, and the point under the cursor stays under it
# (`_pivot_at`, `zoom_at`), so a dozen or so notches on a structure make it readable. While the
# rig follows something the wheel zooms about the focus, which stays on what is followed. A
# wheel that reports a fractional notch (`factor`, a precision wheel or a touchpad) moves the
# camera by that fraction. `client_check.gd`'s `_check_the_wheel` measures it through the real
# camera at the owner's window.

extends Node3D

## Pan speed over the ground, in voxels per second at the default zoom.
##
## PLACEHOLDER: camera speeds and easing — OWNER, ruled at the demo to stand for now.
## They are Tuning, and walled floats (decisions-log item 126 (2) (b); register D-21).
const PAN_RATE := 90.0
## How much one wheel notch zooms, as a factor on the distance. PLACEHOLDER, as above.
const ZOOM_STEP := 1.3
## Radians of turn per pixel of mouse travel. PLACEHOLDER, as above.
const TURN_RATE := 0.005
## How quickly the camera closes on what it follows, per second. PLACEHOLDER, as above.
const FOLLOW_RATE := 4.0
## The nearest and farthest the camera sits from its focus, in voxels.
## PLACEHOLDER, as above.
const NEAREST := 12.0
const FARTHEST := 900.0

## The point the camera looks at.
var focus := Vector3.ZERO
## The heading, in radians about the vertical. The starting pose (this, the pitch and the
## distance) is a PLACEHOLDER, as above.
var yaw := 0.8
## The look-down angle, in radians; negative looks down. PLACEHOLDER, as above.
var pitch := -0.9
## How far the camera sits from its focus. PLACEHOLDER, as above.
var distance := 420.0
## What the camera follows, or null for free-look.
var target: Node3D = null
## The ground under a ray, for the wheel to zoom on: called with the ray's origin and normal,
## answers `{"hit": bool, "at": Vector3}`. The vista binds it to the view's own pick.
var ground_under := Callable()

@onready var camera: Camera3D = $Camera


func _ready() -> void:
	_place()


## Looks at the whole of a map `extent` voxels across (x, up, north), from above its
## north-east corner (largest x and north nearest the camera).
func frame_extent(extent: Vector3) -> void:
	# A little past the centre towards the camera, so perspective does not cut the near
	# corner off the bottom of the frame.
	focus = Vector3(extent.x * 0.58, extent.y * 0.25, extent.z * 0.58)
	distance = maxf(extent.x, extent.z) * 1.5
	yaw = 0.8
	pitch = -1.0
	target = null
	_place()


## Starts or stops following `node`.
func follow(node: Node3D) -> void:
	target = node


func _process(delta: float) -> void:
	if target != null and is_instance_valid(target):
		var weight := 1.0 - exp(-FOLLOW_RATE * delta)
		focus = focus.lerp(target.global_position, weight)
	else:
		target = null
		var move := _key_move()
		if move != Vector3.ZERO:
			var scale_now := distance / 420.0
			focus += move.rotated(Vector3.UP, yaw) * PAN_RATE * scale_now * delta
	_place()


## The free-look keys held down, as a direction in the rig's frame; nothing while a GUI
## control has keyboard focus.
func _key_move() -> Vector3:
	var move := Vector3.ZERO
	if _keys_taken():
		return move
	if Input.is_key_pressed(KEY_W) or Input.is_key_pressed(KEY_UP):
		move.z -= 1.0
	if Input.is_key_pressed(KEY_S) or Input.is_key_pressed(KEY_DOWN):
		move.z += 1.0
	if Input.is_key_pressed(KEY_A) or Input.is_key_pressed(KEY_LEFT):
		move.x -= 1.0
	if Input.is_key_pressed(KEY_D) or Input.is_key_pressed(KEY_RIGHT):
		move.x += 1.0
	if Input.is_key_pressed(KEY_E):
		move.y += 1.0
	if Input.is_key_pressed(KEY_Q):
		move.y -= 1.0
	return move


## Whether a GUI control holds keyboard focus, in this viewport or in an embedded window
## open in it (the editor's Load and Save dialogs), so the keys are text, not moves.
func _keys_taken() -> bool:
	var viewport := get_viewport()
	if viewport.gui_get_focus_owner() != null:
		return true
	for window in viewport.get_embedded_subwindows():
		if window.gui_get_focus_owner() != null:
			return true
	return false


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.pressed:
		# A press on the view that no control took gives the keys back to the camera.
		get_viewport().gui_release_focus()
		if event.button_index == MOUSE_BUTTON_WHEEL_UP:
			zoom_at(event.position, -notches_of(event))
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			zoom_at(event.position, notches_of(event))
	elif event is InputEventMouseMotion and Input.is_mouse_button_pressed(MOUSE_BUTTON_RIGHT):
		yaw -= event.relative.x * TURN_RATE
		pitch = clampf(pitch - event.relative.y * TURN_RATE, -1.5, -0.1)


## How many notches one wheel event is: its `factor` where the platform reports one, a whole
## notch where it reports none (0).
static func notches_of(event: InputEventMouseButton) -> float:
	return event.factor if event.factor > 0.0 else 1.0


## `notches` of the wheel at the screen point `at`, in by a negative count and out by a positive
## one. With the camera's turn unchanged, moving the focus towards the pivot by the share the
## distance changes keeps the pivot where it is on screen.
func zoom_at(at: Vector2, notches: float) -> void:
	var wanted := clampf(distance * pow(ZOOM_STEP, notches), NEAREST, FARTHEST)
	if target == null or not is_instance_valid(target):
		var pivot := _pivot_at(at)
		focus = pivot + (focus - pivot) * (wanted / distance)
	distance = wanted
	_place()


## What the wheel zooms on at the screen point `at`: the ground the view has there; with none
## there (the sky, or a view not yet whole), the level plane through the focus; and the focus
## itself when the ray never meets that plane.
func _pivot_at(at: Vector2) -> Vector3:
	var origin := camera.project_ray_origin(at)
	var normal := camera.project_ray_normal(at)
	if ground_under.is_valid():
		var picked: Dictionary = ground_under.call(origin, normal)
		if picked.get("hit", false):
			# On the ray itself, at the ground's depth: the pick is the middle of a voxel, which
			# is off the ray by up to half a voxel, and that would show at the closest zoom.
			return origin + normal * normal.dot(picked["at"] - origin)
	var crossing: Variant = Plane(Vector3.UP, focus).intersects_ray(origin, normal)
	return crossing if crossing != null else focus


func _place() -> void:
	if camera == null:
		return
	var back := Vector3(0.0, 0.0, 1.0).rotated(Vector3.RIGHT, pitch).rotated(Vector3.UP, yaw)
	camera.global_position = focus + back * distance
	camera.look_at(focus, Vector3.UP)
