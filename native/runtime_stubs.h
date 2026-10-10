#pragma once

#include <string>
#include <cstdint>
#include <vector>
#include <mutex>
#include <utility>

// ITGmania's singleton managers and diagnostics are shared by every oracle.
// Hold this for the entire native call, including manager teardown.
std::mutex& harness_native_mutex();

// Serialized oracle clock, in the same integer microseconds as ArchHooks.
// A capture restores its previous value before releasing the native mutex.
int64_t harness_native_time();
void harness_native_time(int64_t microseconds);

struct lua_State;
void harness_register_lua_globals(lua_State* state);
void install_option_queries(lua_State* state);
void clear_option_queries(lua_State* state);
struct SongLuaModels;
SongLuaModels* install_song_models(lua_State* state);
void destroy_song_models(SongLuaModels* models);

void harness_configure_font_paths(const std::string& theme_fonts,
                                  const std::string& fallback_fonts);
void harness_clear_diagnostics();
std::vector<std::string> harness_take_diagnostics();
// Resolve the actual registered native texture bound by a display command.
// Zero is an unbound texture; unknown nonzero handles are capture errors.
class RageTexture;
RageTexture* harness_texture_for_handle(uintptr_t handle);
// Serialized control scope only; regular actor captures retain metadata loads.
void harness_native_bitmap_loading(bool enabled);
// Use the actual file loader for source dimensions, including first-frame GIFs.
std::pair<int, int> harness_texture_source_size(const std::string& path);
