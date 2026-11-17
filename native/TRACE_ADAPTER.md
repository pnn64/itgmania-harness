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
`NoteDisplay.cpp`, `CubicSpline.cpp`, `Actor.cpp`, and the `RandomNumbers` header
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
