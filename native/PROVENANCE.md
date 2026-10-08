# Native scaffold provenance

`runtime_stubs.cpp` began as a copy of
`../itgmania-reference-harness/src/itgmania_stubs.cpp` from workspace commit
`e329d8570a10db47d71fb34c29971aac18679b25`. It is now owned and evolved by
this harness. The original harness is MIT licensed; its license is retained in
the project root.

The stub file supplies process-level objects and unrelated symbols assumed by
the selected ITGmania translation units. It is not the source of parsed or
calculated baseline fields. Those fields must continue to come from the
ITGmania sources selected in `build.rs` and called by `oracle_bridge.cpp`.

Font and song-Lua texture probing are the explicit exception for texture metadata. The harness has
no display backend, so its headless `RageTexture` reads PNG IHDR or JPEG frame
header dimensions and
applies the dimension steps used by ITGmania's `RageBitmapTexture`: resolution
hints, `doubleres`, maximum size, power-of-two backing dimensions, and frame
rect creation. ITGmania's compiled `Font.cpp`, `FontCharAliases.cpp`, and
`FontCharmaps.cpp` remain responsible for imports, mappings, widths, advances,
shifts, and glyph texture rectangles. Font JSON exposes source, image, and
texture dimensions, and the CLI rejects unsupported font image formats rather than hiding
the limitation.

Song-Lua image handles use this same PNG/JPEG metadata implementation through
`_ITG_TEXTURE_INFO`. Source and frame sizes follow the compiled `RageTexture`
accessors; a copied handle keeps its source path after the original Sprite
loads another image. The host retains each Sprite's texture binding changes
alongside projected geometry, including aliases from invisible resource
caches. This remains a headless metadata probe, without bitmap decoding or a
GPU backend. Movie geometry uses the existing container-header probe; other
image formats are not covered by this path.

Named ActorFrameTexture resources follow the local ActorFrameTexture::Create
registration rule in the Lua host. Sprite texture properties, SetTexture and
Load resolve the same named handle and source dimensions, so projected samples
retain these drawables too. This models resource identity and geometry; it does
not allocate a native GPU render target or prove the captured texture pixels.

Song-Lua discovery wire version 2 includes Song::GetSpecifiedLastSecond and
GetSpecifiedLastBeat from the linked native parser/timing implementation.
Baseline capture ends at the later of its note endpoint and that authored
LASTSECONDHINT, retaining post-note outro callbacks. Explicit headless micro
contexts keep their requested endpoints. This covers the authored song endpoint,
without simulating the ScreenGameplay transition or an audio device.

JPEG probing skips bounded APP and table segments and reads the SOF source
dimensions before entropy-coded data begins. Baseline and progressive JPEG
fixtures check source and backing dimensions, filename sprite-sheet grids,
projected corners, and rejection of truncated segment bounds. The local Save
Your Tears background is 3224x2240. This remains header metadata, not bitmap
decoding or rendered image pixels.

Projected song-Lua samples also retain current left, right, top, and bottom
crop values. The existing world/clip corners remain the uncropped actor plane;
draw comparisons apply the native crop before the perspective divide and
exclude sprites fully cropped away, as `Sprite::DrawTexture` does. Crop changes
invalidate the projection signature even when pose and color remain fixed.

Current shadow offsets and colors are retained separately too. Actor owns
these fields outside TweenState, so setters apply immediately after sleep or
linear commands. Sprite draws the shadow using TranslateWorld before the
diffuse pass; offset comparisons keep world translation separate from actor
zoom and rotation, following the local Actor and Sprite implementations.

AVI geometry reads the video stream's BITMAPINFOHEADER source dimensions,
including negative top-down heights. It skips audio stream formats and movie
payloads and validates RIFF chunk bounds. The local Bad Apple AVI (320x240)
and the generated audio-first/top-down AVI fixtures (2x2) were independently
checked with ffprobe. This establishes source geometry, not decoded pixels or
movie texture allocation dimensions.

Boolean PlayerOptions setter events retain linked native getter values before
and after each call, plus whether the second argument requests chaining.
The Rust semantic audit compares these observations with actual chronological
DeadSync Lua calls. Direct Song-level turns/transforms are checked as option
state because native Player loads Stage transforms into NoteData separately.

Invalid indexed confusionoffset/movex/movey strings retain explicit native
no-op evidence. PlayerIsUsingModifier's copy-and-compare path establishes that
applying the string leaves linked option values unchanged; direct native
getters then snapshot the affected global/active-column fields. The audit uses
these fields instead of treating a requested invalid index as a render target.
A local fixture checks both index 0 and index 17 against existing nonzero
values. Captures without this evidence do not silently omit invalid writes.

Custom ActorFrame draw callbacks now execute after every chronological update.
Explicit draws retain their order, independent actor poses, texture identities,
and render-target Begin/Finish boundaries. ActorMultiVertex.cpp is included
from the unchanged local reference tree; its Lua SetVertices/SetDrawState
bindings and actual
DrawPrimitives submit local vertices, UVs, topology and uint8_t colors to the
existing capturing display. The host applies the recorded draw-stack matrices
to those native local vertices. This covers directly assigned per-draw meshes;
it does not establish AMV geometry tween interpolation or GPU framebuffer pixels.
A micro-fixture retains three distinct color passes across 121 frames and an
explicit Player draw after restoring the screen render context.

Render-target begin records retain the explicit preserve argument separately
from ActorFrameTexture::EnablePreserveTexture. RageTextureRenderTarget.cpp
defaults the former to false. Source/image sizes remain logical; backing sizes
follow the checked-out RageDisplay_OGL.cpp FBO power-of-two allocation, and the
Lua texture getters expose that backing size. Begin records also retain alpha,
depth and float requests. These are allocation semantics from the local source,
not evidence that a GPU supports or actually used the requested float format.
The manual-draw micro-fixture checks both preserve arguments across its frames.

Screen child enumeration includes the modeled engine LifeP1/P2, ScoreP1/P2,
and StepsDisplayP1/P2 alongside the existing player and layer actors.
ScreenGameplay.cpp establishes their names; the checked-out
Simply-Love-SM5/metrics.ini [ScreenGameplay] On commands hibernate them
indefinitely. Explicit draws respect that hibernation, including after
visible(true), while GetVisible still returns true. The draw fixture also
checks that synthetic LifeMeter/LifeMeterBarP1, SongTitle and BPMDisplay aliases
are absent from the initial top-level enumeration. The host still models a
subset of engine children; this is not a complete native ScreenGameplay boot.
The manual-draw fixture also checks that its song root's parent is the
SongForeground layer and that this layer's GetChildren contains that same
root, matching the host's layer attachment before Init/On.

ActorFrameTexture creation also has an independent native fixture. It calls
the linked ActorFrameTexture and RageTexture Lua methods through the reference
LuaBinding.cpp implementation, replacing the former no-op registration and
type-check scaffolding. The capturing display records actual RenderTargetParam
allocations; it still supplies metadata rather than a GPU allocation. The
fixture checks native default size 1x1, integer truncation, frozen dimensions,
names and buffer flags, retained handle equality, strict boolean setters,
invalid/repeated/colliding creation and retry. Native RageTextureID supplies
capture-name normalization, including collisions between collapsed paths.

The serialized texture scaffold now keeps a registry of live source and target
handles with native RageTextureID equality and reference counts. Unloading the
last handle removes its entry before destruction. This supports native creation
collision checks; it does not implement the full texture policy/GC system.
The Lua host freezes its allocated target metadata and skips uncreated targets
and their children. Preservation remains a live draw option. The shared Lua
fixture independently checks metadata, late creation and ScriptError dispatch
in the host and DeadSync.

ScriptError reporting broadcasts native MessageManager messages with their
message parameter and a recursion guard. LuaTable construction and key access
now preserve the native parameter-table and stack behavior, replacing their
no-op stubs. The native fixture observes the error payloads, verifies stack
balance and repeats creation to check registry and listener cleanup. The
isolated AMV metadata adapter suppresses only the setters' ignored self return,
which would otherwise export a manager-owned actor table into another Lua
registry. Native calls in the manager state keep the original self return.

Current-song backgrounds resolve the native Song::GetBackgroundPath resource.
The fallback background sizing methods use PrefsManager's CoverPreserve default
and the checked-in 02 Actor.lua fit formulas. The existing native actor sizing
fixture verifies moving and nonmoving calls against drawn geometry.

The manual-player-draws actor fixture supplies an independent local geometry
golden for whole-Player replay. Linked native Actor/ActorFrame/Sprite and
ActorFrameTexture traversal draw twenty-four quads across three root poses, two
wrapper states, a transformed caller, a translated screen root, Player and
field cameras, and a capture whose Begin resets the caller stack. Underlay
markers remain separate from the Combo/NoteField/Judgment subtree specified by
Player.cpp and Simply Love's ComboUnderField metric. The local Simply Love
ScreenGameplay in/default.lua retains Stage/Event text after its lead-in ends;
separate In markers test ordinary and explicit screen-child placement. These
markers test geometry and ownership, not native text layout or glyph pixels.
The field-camera and tilt subtree are specified from Player.cpp's
PlayerNoteFieldPositioner, including uniform XYZ
zoom and the GrayArrows midpoint pivot. The native capture retains homogeneous
clip coordinates, so zero zoom in the depth axis remains testable without
inverting a singular actor pose.

DeadSync's production Lua compiler and frame compositor compare their main and
capture geometry with this golden. A separate harness test regenerates native
draws and compares all twenty-four named sprites. Real gameplay layer values in
the compositor test also check that separate explicit draws retain their local
child order instead of being interleaved by a global layer sort. This fixture
boots native actors, not an actual Player or ScreenGameplay; it does not
establish judgment/combo subtree snapshots, complete gameplay-screen contents,
or full-chart pixels.

Separate local runtime diagnostics on 2026-10-06 boot the installed ITGmania
1.3.0 ScreenGameplay through beat 16 with one and two players. They use a muted,
portable temporary install and the supplied Sharkmode resources. The installed
Simply-Love-SM5 metrics, lead-in and Underlay scripts match the checked-out
reference hashes. The two-player diagnostic uses versus with both players
autoplaying the existing dance-single Challenge chart. Its actual top-level
inventory has 28 distinct names plus unnamed groups; the semantic host currently
models 13 names. These diagnostic traces are local investigation data, not
published headless baselines or full-chart framebuffer evidence. The game
installation and reference source tree remain unchanged.

Hibernation in the semantic host now calls the linked `Actor::Update` through
`_ITG_HIBERNATE_STEP`. Its adapter records whether `UpdateInternal` runs,
the remaining native float sleep, and the adjusted delta on the wake-up
frame. The host stops the actor, wrappers, children, effect timer, tween
queue, and update callback while that native phase is blocked. It preserves
`Actor::GetVisible`, applies the same sleep to ordinary and explicit draws,
and includes sleep plus child queues in `ActorFrame::GetTweenTimeLeft`.
The previous absolute draw-only deadline is removed. The hibernation
fixture checks a wake-up between canonical frames, paused child alpha,
paused aux/getter reads, the callback's leftover delta, queue time, and
unchanged own visibility. Captures before harness 0.1.6 that execute a
positive hibernate require regeneration; passing against their unpaused
tweens is not evidence of native actor-update parity.

`SetUpdateRate` and `GetUpdateRate` now retain the native float rate.
`ActorFrame.cpp::UpdateInternal` multiplies that rate after hibernation and
wrapper updates and before its own tween/effect work, children, and callback.
The host performs that multiplication through the linked float conversion.
The scaled hibernation regression covers an unscaled wrapper, a doubled
owner delta, nested child rates, getters, and resulting native tween alpha.
Earlier captures that execute a non-default `SetUpdateRate` require
regeneration with harness `0.1.7` or newer.

ITGmania itself is a separate work with its own license in `vendor/itgmania`.
Simply Love is pinned separately in
`vendor/simply-love` and retains its own license. See the source pins and update
workflow in the project README.

Harness 0.1.16 preserves each newly enqueued segment's `queue_start_seconds`.
The semantic host sums only the actor's own pending native float durations
and hibernation before appending, following `Actor::BeginTweening` and
`Actor::GetTweenTimeLeft`; ActorFrame's child maximum is not its own queue.
The value is source-derived headless timing, not native queue memory. The
`message_queue_offsets_match_native` regression independently runs linked C++
Actor queues, dispatches a command while an earlier tween remains queued,
and compares all three appended segment offsets and durations. Sleep's
implicit zero-duration tail retains the offset after the sleep. Older
captures lack these offsets and must be recaptured to audit messages that
append to an existing queue.
