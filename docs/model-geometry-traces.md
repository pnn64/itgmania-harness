# Native Model geometry observations

Song traces use `model_geometry_encoding: "column-buffer-v1"` to share
identical arrays without dropping frames, vertices, or draw passes. The
independent `actor-conformance` runner continues to return full native
vertices, so it can verify reconstruction of the song representation.

`model_geometry_buffers` is a document-wide array of immutable arrays of
numeric vectors. Buffer IDs are one-based. A primitive contains:

* `vertex_count`: the number of triangle vertices, in native draw order.
* `vertex_buffers`: IDs for `local`, `world`, `view`, `clip`, `ndc`, `screen`,
  `uv`, `transformed_uv`, and `color`. Each column has `vertex_count` rows.
* `normals_buffer` and `texture_matrix_scale_buffer`: IDs for the two native
  mesh attribute arrays, also with `vertex_count` rows.

To reconstruct vertex `i`, read row `i` from every vertex column under its
original field name. Read the two mesh attribute buffers under `normals`
and `texture_matrix_scale`. Remove the count/reference fields to obtain the
original full primitive. All remaining metadata, including native material,
texture matrix, mesh index/name, lighting, culling, depth, viewport, blend,
and texture mode, remains on each primitive.

This encoding applies to both ordinary Model tracks and explicit Model
draws in `manual_draw_frames`. ActorMultiVertex data retains its original
format. Invisible samples and empty primitive lists remain explicit.
Consumers must reject unknown encodings, absent columns, invalid buffer
IDs, and inconsistent row counts; the capability flag alone is insufficient
evidence of full-song parity.

The single capture session owns the interning index. Keys use the same
17-digit numeric representation as trace JSON, with exact row boundaries;
no quantization or hash collision can merge different observations. The
index is capped at 65,536 keys and 64 MiB of serialized key bytes. On
saturation, new observations remain in the output without being indexed;
existing indexed values can still be reused. There is no frame-loop pruning.
The immutable output pool is reference data and retains every distinct
observation needed by the complete chart.

`model_geometry_buffer_stats` records output buffers, indexed buffers, key
bytes, hits, and saturated misses. This is a storage change, not a change to
the native loader, update, draw, projection, or comparison tolerances.
Whole-song completeness still requires actual production comparisons of
every supported Model observation.
