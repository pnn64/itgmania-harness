#pragma once

#include <string>
#include <cstdint>
#include <vector>
#include <mutex>

// ITGmania's singleton managers and diagnostics are shared by every oracle.
// Hold this for the entire native call, including manager teardown.
std::mutex& harness_native_mutex();

struct lua_State;
void harness_register_lua_globals(lua_State* state);
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
