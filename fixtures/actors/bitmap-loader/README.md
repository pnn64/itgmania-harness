These small file controls execute ITGmania's pinned image loaders and
RageBitmapTexture upload preparation. The request is `../bitmap-loader.json`.
PNG inputs with existing names are exact copies of DeadSync's native surface
controls; BMP/GIF inputs preserve duplicate pink palette entries and indices.
The GIF also supplies a transparent white entry. The JPEG is a uniform 8x8 RGB
image encoded once with Pillow 12.3.0, quality 95, no chroma subsampling; decoding
uses pinned native libjpeg-turbo. No source image is generated during tests.

`native-png16` retains an RGBA16 source whose first pixel has samples
0x1234, 0x80FF, 0xFEFF, 0xFFFF: native decoding strips the low byte.
The tests assert independently known input colors and selected native policy
branches, repeat the oracle to check determinism, and exercise rejected inputs
followed by AFT recreation to detect leaked texture registry entries.
The pinned BMP4 loader reads low nibbles first; the tests retain that native
ordering. Alpha-map palettes use the native requested four alpha bits, so an
input alpha of 128 maps to 136.

Sprite upload pixels are deliberately absent because the unused allocation
padding is not fully initialized by native Blit. Model uploads stretch across
the allocation and expose exact pixels. Neither output claims GPU conversion,
mip generation or framebuffer parity.
