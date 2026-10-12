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

Use `Program/ITGmania-harness-trace.exe` with `--itgmania-bin` on Windows.
The stock executable remains intact for independent checks. Captures advertise
`native_c_method_hooks` when the utility is present. Older executables keep
the Lua hook; any lost tail-call provenance remains explicit and cannot
prove parity. Modifier return tuples and canonical player-number paths are
recorded separately from this capability. The returned player-state object's
original `GetPlayerNumber` binding supplies its identity; the caller's argument
representation does not define a different player.
