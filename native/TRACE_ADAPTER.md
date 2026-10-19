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

Reconfigure after changing the private source revision. The adapter stamps its
own Git revision, tracked source modification state, and SHA-256 hashes of
`NoteDisplay.cpp` and `CubicSpline.cpp` into the executable. Notefield traces
retain these in `input.native_source`; the collector rejects missing provenance.
The CLI's pinned oracle revision does not identify an external trace executable.

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
