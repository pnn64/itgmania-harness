The full ITGmania song Lua recorder can use `trace_adapter.cpp` to retain
the caller of native bindings through Lua 5.1 tail calls. A Lua wrapper
alone replaces that caller with a tail-call pseudo-frame. Filtering that
pseudo-frame as an unrelated caller loses authored calls from the capture.

The adapter adds `ITGMANIA_HARNESS_WRAP(observer)`, a C closure that invokes
the observer in the same Lua VM and preserves all arguments, return slots,
object identities, and Lua errors. It changes no gameplay or scoring code.
The recorder verifies nil slots, object identity, and error propagation at
startup, and reuses shared function hooks to preserve method identity.

Build the utility into a private ITGmania build without editing the reference
tree. Configure the existing build with:

```
cmake -S <private-source> -B <build> -DCMAKE_PROJECT_INCLUDE=<harness>/native/trace_adapter.cmake
cmake --build <build> --config Release --target ITGmania
```

On a fresh Linux build, first run `cmake --build <build> --target libusb`.
The pinned engine links the bundled static library without declaring that
target dependency; building only `ITGmania` can otherwise fail at the link step.

Reconfigure after changing the private source revision. The adapter stamps its
own Git revision, tracked source modification state, and SHA-256 hashes of
`NoteDisplay.cpp`, `CubicSpline.cpp`, `Actor.cpp`, `ScreenGameplay.cpp`, and the `RandomNumbers` header
and implementation into the executable. Semantic gameplay
and notefield traces retain these in `input.native_source`; the collector rejects
missing provenance. Native archive manifests use this stamp when available.
The CLI's pinned oracle revision does not identify an external trace executable.
Build from the pinned revision when comparing against pinned-reference behavior.

Interactive traces from 0.1.103 retain `step_clock_inputs` for each input dispatch.
These are the original `Player::Step` timer reads, position and music rate,
observed before note selection and scoring. The Lua event's later `DeviceInput.ago`
cannot reconstruct that clock. A scoped observer keeps separate P1/P2 reads in
routine mode and excludes autoplay calls outside physical input dispatch.
Player and column observations let consumers check their independent routing;
they are not instructions to select a note or import a judgment. Repeat, redirected,
and non-step events have no Step clocks. Interactive capture requires the
`native_input_step_clocks` capability and rejects outdated native executables.

Use `Program/ITGmania-harness-trace.exe` with `--itgmania-bin` on Windows.
For headless captures the collector sets `ITGMANIA_HARNESS_HEADLESS`. The
observed Windows crash handler saves the native `crashinfo.txt` and exits
without opening its dialog. The collector reports that file immediately.
After a Windows Release build, build the `itgmania-harness-crash-symbols`
target to retain the matching `.vdi` sidecar beside the renamed executable.
The stock executable remains intact for independent checks. Captures advertise
`native_c_method_hooks` when the utility is present. Older executables keep
the Lua hook; any lost tail-call provenance remains explicit and cannot
prove parity. Modifier return tuples and canonical player-number paths are
recorded separately from this capability. The returned player-state object's
original `GetPlayerNumber` binding supplies its identity; the caller's argument
representation does not define a different player.

Linux X11 builds also support `--headless --drawable-samples` and
`--notefield-samples` with the real GLX renderer. The observed window source
creates its native GLX context without mapping the context window and skips
the associated `MapNotify` wait. Fullscreen and borderless requests are rejected
in this mode. `ITGMANIA_HARNESS_WINDOW_VISIBLE` queries the actual X11 window
attributes; either mapped state rejects offscreen capture, and an unavailable
query is an error. The adapter does not replace native draw, Model, or AFT code.
This mode still needs an X server and working OpenGL; on a server use an isolated
Xvfb display, for example:

```
xvfb-run -a -s '-screen 0 1280x720x24 -nolisten tcp' \
  itgmania-harness-rs song-lua-trace song.ssc \
  --itgmania-bin /path/to/ITGmania-harness-trace \
  --headless --drawable-samples --until-gameplay-end --out native.json
```

The same display wrapper can run the existing ignored
`native_notefield_capture_includes_models_and_position_caps` control with
`ITGMANIA_HARNESS_TRACE_BIN` set to that executable. It checks real note models,
hold strips, receptor visits, material state, native clocks, and hidden-window
evidence. Dedicated notefield output remains a separate render observation;
ordinary drawable captures retain semantic event replay.

The recorder also stores `actor_identity_inputs` before existing authored
`InitCommand` or `OnCommand` callbacks. It calls the native metatable's C
`Luna<T>::tostring_T` binding to observe the object pointer as hexadecimal text,
avoiding Lua number precision loss. No command is added to the actor. Each
input is anchored to the following startup event and contains no callback
results or message delivery order. Actors without an observed startup callback
have no identity input; their identities must never be inferred from expected
message events. These inputs help diagnose `MessageManager`'s native pointer
ordering without treating allocator-dependent callback permutations as passes.

`native_declaration_ancestry` preserves declared ancestors of actors observed
at runtime, including containers with no authored startup callbacks. The
exported forest has no dangling parent or child references. Unbound ancestors
keep empty `runtime_actors` arrays and receive no invented pointer input.
Child declarations follow `XmlFileUtil::XNodeFromTableRecursive` traversal,
including entries beyond nil holes. An explicit `children` table takes
precedence over direct array children, as in `ActorFrame::LoadChildrenFromNode`.
Declaration trees with no observed descendants remain excluded. Paths do not establish
complete physical ownership: foreground root names are overwritten by
`Foreground::LoadFromSong`, and reused definitions still require instance
ownership evidence before replay mapping.

Drawable semantic captures from 0.1.105 require `native_render_random_inputs`.
The observer retains the three original f32 `randomf(-1, 1)` returns in
`Actor::PreDraw`'s vibrate branch, before multiplication by effect magnitude
and zoom. It returns those same values to the native expression and does not
seed, replace, or consume the engine PRNG. That global `std::mt19937` is seeded
from native startup time, independently of the Lua PRNG controlled by
`random_seed`. Its seed cannot be reconstructed from the Lua seed. Exact
replay therefore uses these observed external random inputs.

`input.render_random` identifies protocol 1, the observation site, the native
generator/distribution, the default time seed source, and the association
`active-Actor-Draw-PreDraw-scopes`. The matching `input.native_source` hashes
are `actor_sha256`, `random_numbers_sha256`, and `random_numbers_header_sha256`.
`render_random_inputs` records contiguous `sequence`, captured `scene` and
`update_sequence`, owner `actor`/`definition_id`/`path`, optional source identity
`source_actor`/`source_definition_id`/`source_path`, and raw XYZ `values`.
`wrapper_index` is 0 for the owner's own PreDraw, positive for its native
one-based wrapper slot, and -1 for its fake parent. A wrapper can be identified
by its independently bound owner and slot even if it has no runtime actor
record; self and fake-parent scopes require a source actor identity.

Each input retains `native_scope_sequence`, `native_draw_sequence`,
`native_update_sequence`, `draw_ordinal` (per owner per native screen update),
and `predraw_ordinal` (within that actual Draw). Native update counters are
separate from probe frames: an immediate Draw can occur before the gameplay
probe records the next captured frame. RAII scopes keep ancestor inputs alive
through descendants and remove them when their Draw returns. The recorder
assigns input sequences on first use by a declared drawable; it excludes
unrelated theme draws and inputs from scopes that submit no captured geometry.
Every drawable carries ordered `render_random_sequences`, sharing ancestor
IDs while distinguishing wrappers and repeated Draw calls. These are input
occurrences, not transform output. ActorFrameTexture's matrix reset does not
end native Draw lifetimes; consumers must apply their own render-target and
ancestry rules when computing geometry.

The validator requires finite exact f32 triples in [-1, 1], matching runtime
identities and scene/frame anchors, unique draw roles, complete scope lists,
zero `dropped_render_random_inputs`, and no unused inputs. Each drawable frame
also carries a cumulative `render_random_inputs` count. Unknown vibrating
ancestors of captured song draws fail observation instead of being omitted.

Reconfigure and rebuild the executable and Windows crash-symbol target, then
rebuild the Rust collector. Run the focused validator tests with
`cargo test --release render_random_capture`. The native regression
`native_render_random_scopes_preserve_parent_wrapper_and_repeated_draw_inputs`
requires `ITGMANIA_HARNESS_TRACE_BIN` and runs with `--ignored --nocapture`;
it covers shared screen/parent jitter, repeated draws, wrappers, fake parents,
and zero effect magnitude. Existing traces without raw inputs cannot acquire
this capability by conversion. Recapture with the original song, chart, theme,
player options, timing and drawable settings, retaining the old trace and its
archive separately until the new capture has passed replay.

Render-target pass protocol 1 observes `RageTextureRenderTarget` after the
native target and matrices are switched or restored. It records automatic
ActorFrameTexture passes and manual RageTextureRenderTarget calls in the
semantic drawable profile. `after_native_draw_sequence` is the latest native
submission at that boundary. Equal cursors retain event order, including empty
or nested passes. The collector checks the target stack and the active target
for every retained drawable and rejects dropped events.

Notefield captures from 0.1.109 require OpenGL Model render-state protocol 1.
The generated `RageDisplay_OGL.cpp` reads the active context immediately before
`RageCompiledGeometry::Draw`, after the native Model has selected its material
and texture pass. `state.render_state` retains the native `SetMaterial` RGBA
arguments, current GL color, lighting, blend factors/equations, depth function,
write mask and range, culling, alpha test, and each texture unit's binding and
environment mode. `textures[].mode` is the actual native `SetTextureMode`
argument: 0 is Modulate, 1 is Glow, and 2 is Add. Other numeric renderer enums
are OpenGL constants. A disabled unit may retain an earlier binding/mode; it
does not claim an active texture. Active resource filenames come from the
Model's actual texture-handle lookup, including animated texture frames.

The capability explicitly names `opengl`; Actor render-state defaults are not
substituted for the final backend state. Current color is meaningful for these
unlit Models; a lit Model instead uses the submitted material and lights.
Material and current-color components retain the native floating-point values,
including values above one. Alpha can be zero even when geometry is submitted.
This observation describes draw inputs, not final pixel coverage or occlusion.
The native model control exercises separate diffuse/glow passes, a fading
material, a zero-alpha material, and Model's blend override before glow.

Notefield captures from 0.1.110 also require `native_option_clock_deltas`.
Every `update_frames` entry retains `option_deltas = {song, players: [P1, P2]}`
from the existing native `SongOptions::Approach` and `PlayerOptions::Approach`
observer. These are the exact nonnegative f32 inputs supplied by each
`ModsGroup`'s own `RageTimer`, not the Screen update delta or a music-clock
difference. `GameState::Update` advances SongOptions and both PlayerStates
before Screen updates, so the gameplay overlay samples the values already
used by that frame's geometry. The observer does not read or reset those
timers. Export preserves all three inputs independently and rejects missing,
malformed, or non-f32 tuples when the capability is claimed. Retained older
traces keep their absent capability and must be recaptured for this evidence.

`native_gameplay_clock_inputs` observes the original f32 returned by
`ScreenGameplay::m_timerGameplaySeconds.GetDeltaTime()` in `STATE_DANCING`,
after the screen has updated its authored actors and checked failure state.
The generated source passes that value to the observer immediately after the
existing read. It does not read or reset the timer again, and does not export
accumulated statistics as replay inputs. `input.native_source.screen_gameplay_sha256`
identifies the original source containing that call.

Every `update_frames` row carries `native_update_sequence`, the native monotonic
ScreenManager update counter, and an explicit `gameplay_clock_inputs` value.
The value is null when that update made no timer call, including intro, pause,
and outro updates. Otherwise it contains `delta_seconds`, the phase
`after-gameplay-actors`, and matching `scene`, `native_update_frame`, and
`native_update_sequence` associations. `music_rate` and `steps_time_active` are
diagnostics from the native rate and first-beat-inclusive/last-beat-exclusive
gate at that point. Replayers must compute their own rate, failure state,
gate, and f32 accumulations; these diagnostic fields are not replacements for
owned gameplay state. The overlay's stage observation sees accumulated values
through the preceding update, because this timer call occurs later.

The native Lua encoder cannot retain an object key with a nil value, so probes
write false for the explicit no-call state. The collector converts only that
sentinel to JSON null before validation and export. Missing keys remain invalid
when the capability is present, and retained older captures do not gain this
capability by conversion. The validator rejects stale frame associations,
duplicate native timer occurrences, malformed phases, and non-f32 values.
Stage samples retain `steps_seconds` alongside gameplay/alive observations and
identify their latest observed update. These expected observations stay separate
from the raw timer channel.

Run `cargo test --release native_gameplay_clock_inputs` for schema validation.
The ignored `native_gameplay_clock_inputs_reproduce_stage_observations` control
requires `ITGMANIA_HARNESS_TRACE_BIN`. It records a real pause/resume and music
rate change with an independent fixed actor delta, then checks exact f32
gameplay, steps, and alive observations against the preceding timer inputs.
