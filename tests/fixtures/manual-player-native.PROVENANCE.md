# Manual Player native geometry fixture

`manual-player-native.json` is copied unchanged from
`../deadsync/tests/fixtures/itgmania-song-lua-micro/manual-player-native.json`.
It was captured by the embedded native oracle for
`fixtures/actors/manual-player-draws.json`.

Its original provenance identifies workspace commit
`5b205125ad53b9867bb4a494ff858f8d38ad4406`, which contained a copied ITGmania
tree rather than an independent ITGmania checkout. The source, bundled Lua,
JSONCPP, PCRE, miniz, noteskins, and fallback resources used by that workspace
match upstream ITGmania v1.2.0 (`5c737928d93778c2c9f68e276052b220b43a468f`)
after normalizing line endings. Its Simply Love tree matches upstream 5.9.0
(`dd06138b15492f4136796dfe4b6708ced0f7b9eb`) the same way.

The integration test compares the captured geometry directly and retains the
original fixture provenance. It does not require the external DeadSync checkout
or equate its workspace revision with the upstream engine revision.
