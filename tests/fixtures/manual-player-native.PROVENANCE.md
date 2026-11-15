# Manual Player native geometry fixture

`manual-player-native.json` stores the 24 named actors' draw arrays from the full
capture in `../deadsync/tests/fixtures/itgmania-song-lua-micro/manual-player-native.json`.
The full capture retains `samples`; this harness fixture retains the historical
`sprites` layout consumed by its integration test. Both were captured by the embedded native oracle for
`fixtures/actors/manual-player-draws.json`.

The fixture was refreshed on 2026-11-15 with native ITGmania
`008bfcfef57a107eaafe08b4de7d9d42c6dd1e2f`, using:

```
itgmania-harness-rs actor-conformance fixtures/actors/manual-player-draws.json
```

The 24 named actors' `draws` arrays become `sprites`, alongside the native
`oracle`, `provenance`, and `screen` fields. Decimal parsing and writing preserve
the captured doubles. Every numeric difference from the previous fixture
preserves the underlying float32 bits; the refresh corrects decimal serialization
drift, with no geometry tolerance or comparator change.

The previous provenance identified workspace commit
`5b205125ad53b9867bb4a494ff858f8d38ad4406`, which contained a copied ITGmania
tree rather than an independent ITGmania checkout. The source, bundled Lua,
JSONCPP, PCRE, miniz, noteskins, and fallback resources used by that workspace
match upstream ITGmania v1.2.0 (`5c737928d93778c2c9f68e276052b220b43a468f`)
after normalizing line endings. Its Simply Love tree matches upstream 5.9.0
(`dd06138b15492f4136796dfe4b6708ced0f7b9eb`) the same way.

The integration test compares the captured geometry directly and retains the
native fixture provenance. It does not require the external DeadSync checkout
or equate its workspace revision with the upstream engine revision.
