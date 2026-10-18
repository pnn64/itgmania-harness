#include "global.h"
#include "interactive_trace.h"
#include "Actor.h"
#include "ActorMultiVertex.h"
#include "BitmapText.h"
#include "GameInput.h"
#include "GameState.h"
#include "PlayerState.h"
#include "InputFilter.h"
#include "InputMapper.h"
#include "LuaManager.h"
#include "RageMath.h"
#include "RageTexture.h"
#include "Screen.h"
#include "ScreenManager.h"
#include "Sprite.h"
#include <chrono>
#include <thread>
#include <map>
#include <vector>

namespace {
Actor* drawing = nullptr;
float option_song_delta = 0;
float option_player_delta[NUM_PLAYERS] = {};
struct CatalogInputs { float offset, rate; };
std::map<const void*, CatalogInputs> catalog_inputs;
bool arrow_query_scope = false;
std::vector<double> arrow_query_clocks;
int arrow_cache_scope = -1;
double arrow_update_seconds = 0;
double arrow_cache_seconds[NUM_PLAYERS][2] = {};
bool arrow_cache_seen[NUM_PLAYERS][2] = {};

int arrow_clock_scope(lua_State* L) {
  if (lua_toboolean(L, 1)) {
    arrow_query_clocks.clear(); arrow_query_scope = true; return 0;
  }
  arrow_query_scope = false;
  lua_createtable(L, static_cast<int>(arrow_query_clocks.size()), 0);
  for (size_t index = 0; index < arrow_query_clocks.size(); ++index) {
    lua_pushnumber(L, arrow_query_clocks[index]); lua_rawseti(L, -2, static_cast<int>(index + 1));
  }
  return 1;
}

void arrow_frame_inputs(lua_State* L) {
  lua_createtable(L, 0, 2);
  lua_pushnumber(L, arrow_update_seconds); lua_setfield(L, -2, "update_seconds");
  lua_createtable(L, NUM_PLAYERS, 0);
  for (int player = 0; player < NUM_PLAYERS; ++player) {
    lua_createtable(L, 0, 2);
    for (int tangent = 0; tangent < 2; ++tangent) {
      if (!arrow_cache_seen[player][tangent]) continue;
      lua_pushnumber(L, arrow_cache_seconds[player][tangent]);
      lua_setfield(L, -2, tangent ? "tan_tipsy_seconds" : "tipsy_seconds");
    }
    lua_rawseti(L, -2, player + 1);
  }
  lua_setfield(L, -2, "players");
}

int current_catalog_inputs(lua_State* L) {
  const auto found = catalog_inputs.find(GAMESTATE->m_pCurSong.Get());
  if (found == catalog_inputs.end()) { lua_pushnil(L); return 1; }
  lua_createtable(L, 0, 3);
  lua_pushnumber(L, found->second.offset); lua_setfield(L, -2, "global_offset_seconds");
  lua_pushnumber(L, found->second.rate); lua_setfield(L, -2, "music_rate");
  lua_pushstring(L, "native-catalog-note-time-calculation"); lua_setfield(L, -2, "initialization");
  return 1;
}

int option_deltas(lua_State* L) {
  lua_createtable(L, 0, 2);
  lua_pushnumber(L, option_song_delta); lua_setfield(L, -2, "song");
  lua_createtable(L, NUM_PLAYERS, 0);
  for (int player = 0; player < NUM_PLAYERS; ++player) {
    lua_pushnumber(L, option_player_delta[player]); lua_rawseti(L, -2, player + 1);
  }
  lua_setfield(L, -2, "players");
  return 1;
}
struct RenderState { bool write = false, wrap = false, filter = true; int test = 0, blend = 0, cull = 0; float bias = 0; } state;

// Observational failures are distinct from errors in authored Lua.
void observe(lua_State* L, int args) {
  if (lua_pcall(L, args, 0, 0) != 0)
    lua_setglobal(L, "ITGMANIA_HARNESS_INTERACTIVE_ERROR");
}
void field(lua_State* L, const char* name, double value) {
  lua_pushnumber(L, value); lua_setfield(L, -2, name);
}
void boolean(lua_State* L, const char* name, bool value) {
  lua_pushboolean(L, value); lua_setfield(L, -2, name);
}
int inject_input(lua_State* L) {
  const int player = luaL_checkinteger(L, 1);
  const char* name = luaL_checkstring(L, 2);
  const bool down = lua_toboolean(L, 3);
  const char* key = luaL_checkstring(L, 4);
  if (player < 1 || player > 2) return luaL_error(L, "player must be 1 or 2");
  const auto button = StringToGameButton(INPUTMAPPER->GetInputScheme(), name);
  const auto physical = StringToDeviceButton(key);
  if (button == GameButton_Invalid || physical == DeviceButton_Invalid)
    return luaL_error(L, "invalid native game button or keyboard key");
  DeviceInput input(DEVICE_KEYBOARD, physical, down ? 1.0f : 0.0f, RageTimer());
  INPUTMAPPER->SetInputMap(input, GameInput(static_cast<GameController>(player - 1), button), 0);
  // The actual InputFilter produces press/repeat/release, and the next normal
  // GameLoop input phase maps and delivers them. No Lua callback is invoked here.
  INPUTFILTER->ButtonPressed(input);
  return 0;
}
int pace_capture(lua_State* L) {
  const double seconds = luaL_checknumber(L, 1);
  if (!(seconds > 0 && seconds <= 1)) return luaL_error(L, "invalid capture frame period");
  static auto previous = std::chrono::steady_clock::now();
  const auto next = previous + std::chrono::duration_cast<std::chrono::steady_clock::duration>(
      std::chrono::duration<double>(seconds));
  std::this_thread::sleep_until(next);
  previous = std::chrono::steady_clock::now();
  return 0;
}
void register_interactive(Lua* L) {
  lua_pushcfunction(L, arrow_clock_scope);
  lua_setglobal(L, "ITGMANIA_HARNESS_ARROW_CLOCK_SCOPE");
  lua_pushcfunction(L, current_catalog_inputs);
  lua_setglobal(L, "ITGMANIA_HARNESS_CATALOG_INPUTS");
  lua_pushcfunction(L, option_deltas);
  lua_setglobal(L, "ITGMANIA_HARNESS_OPTION_DELTAS");
  lua_pushcfunction(L, pace_capture);
  lua_setglobal(L, "ITGMANIA_HARNESS_PACE_CAPTURE");
  lua_pushcfunction(L, inject_input);
  lua_setglobal(L, "ITGMANIA_HARNESS_INJECT_INPUT");
  lua_pushboolean(L, true);
  lua_setglobal(L, "ITGMANIA_HARNESS_INTERACTIVE_AVAILABLE");
}
REGISTER_WITH_LUA_FUNCTION(register_interactive)
}

void itgmania_harness_catalog_inputs(const void* song, float offset, float rate) {
  // Raw configuration at note-time calculation, before theme preference changes.
  // Never expose the stored Song getter values through the input bridge.
  catalog_inputs[song] = {offset, rate};
}

double itgmania_harness_arrow_clock(double value) {
  if (arrow_query_scope) arrow_query_clocks.push_back(value);
  if (arrow_cache_scope >= 0) {
    arrow_cache_seconds[arrow_cache_scope / 2][arrow_cache_scope % 2] = value;
    arrow_cache_seen[arrow_cache_scope / 2][arrow_cache_scope % 2] = true;
  }
  return value;
}

ItgmaniaHarnessArrowCacheScope::ItgmaniaHarnessArrowCacheScope(int player, bool tangent)
    : previous(arrow_cache_scope) { arrow_cache_scope = player * 2 + (tangent ? 1 : 0); }
ItgmaniaHarnessArrowCacheScope::~ItgmaniaHarnessArrowCacheScope() { arrow_cache_scope = previous; }

void itgmania_harness_arrow_update_clock(double value) { arrow_update_seconds = value; }

void itgmania_harness_after_arrow_update() {
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_AFTER_ARROW_UPDATE");
  if (lua_isfunction(L, -1)) { arrow_frame_inputs(L); observe(L, 1); }
  lua_settop(L, top); LUA->Release(L);
}

void itgmania_harness_option_delta(const void* options, float delta, bool song) {
  // Approach receives the independent ModsGroup RageTimer delta. Observe this
  // input without changing the timer, options, or authored Lua results.
  if (!GAMESTATE) return;
  if (song) {
    if (options == &GAMESTATE->m_SongOptions.GetCurrent()) option_song_delta = delta;
  } else {
    for (int player = 0; player < NUM_PLAYERS; ++player) {
      const auto* state = GAMESTATE->m_pPlayerState[player];
      if (state && options == &state->m_PlayerOptions.GetCurrent()) option_player_delta[player] = delta;
    }
  }
}

void itgmania_harness_before_screen_update(float delta) {
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_BEFORE_SCREEN_UPDATE");
  if (lua_isfunction(L, -1)) { lua_pushnumber(L, delta); observe(L, 1); }
  lua_settop(L, top); LUA->Release(L);
}

void itgmania_harness_after_screen_draw() {
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_AFTER_SCREEN_DRAW");
  if (lua_isfunction(L, -1)) observe(L, 0);
  lua_settop(L, top); LUA->Release(L);
}

void itgmania_harness_observe_input(lua_State* L, Screen* screen) {
  if (screen != SCREENMAN->GetTopScreen()) return;
  const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_INPUT_EVENT");
  if (lua_isfunction(L, -1)) { lua_pushvalue(L, top); observe(L, 1); }
  lua_settop(L, top);
}

Actor* itgmania_harness_draw_actor(Actor* actor) {
  Actor* previous = drawing; drawing = actor; return previous;
}

void itgmania_harness_render_state(Actor*, bool write, int test, bool wrap,
    bool filter, int blend, float bias, int cull) {
  state = {write, wrap, filter, test, blend, cull, bias};
}

void itgmania_harness_draw_vertices(const char* primitive, const RageSpriteVertex* vertices,
    int count, const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection,
    const RageMatrix& centering, const RageMatrix& texture) {
  if (!drawing || count <= 0) return;
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_DRAW_VERTICES");
  if (!lua_isfunction(L, -1)) { lua_settop(L, top); LUA->Release(L); return; }
  drawing->PushSelf(L);
  lua_pushstring(L, primitive);
  lua_createtable(L, count, 0);
  for (int i = 0; i < count; ++i) {
    const auto& vertex = vertices[i];
    RageVector4 local(vertex.p.x, vertex.p.y, vertex.p.z, 1), w, v, clip, out;
    RageVec4TransformCoord(&w, &local, &world);
    RageVec4TransformCoord(&v, &w, &view);
    RageVec4TransformCoord(&clip, &v, &projection);
    RageVec4TransformCoord(&out, &clip, &centering);
    RageVector4 uv(vertex.t.x, vertex.t.y, 0, 1), transformed_uv;
    RageVec4TransformCoord(&transformed_uv, &uv, &texture);
    const double values[] = {vertex.p.x, vertex.p.y, vertex.p.z,
        clip.x, clip.y, clip.z, clip.w, transformed_uv.x, transformed_uv.y,
        vertex.c.r / 255.0, vertex.c.g / 255.0, vertex.c.b / 255.0, vertex.c.a / 255.0};
    lua_createtable(L, 13, 0);
    for (int k = 0; k < 13; ++k) { lua_pushnumber(L, values[k]); lua_rawseti(L, -2, k + 1); }
    lua_rawseti(L, -2, i + 1);
  }
  lua_createtable(L, 0, 9);
  boolean(L, "z_write", state.write); boolean(L, "texture_wrapping", state.wrap);
  boolean(L, "texture_filtering", state.filter); field(L, "z_test", state.test);
  field(L, "blend", state.blend); field(L, "z_bias", state.bias); field(L, "cull", state.cull);
  RageTexture* resource = nullptr;
  if (auto* sprite = dynamic_cast<Sprite*>(drawing)) resource = sprite->GetTexture();
  if (auto* mesh = dynamic_cast<ActorMultiVertex*>(drawing)) resource = mesh->GetTexture();
  if (resource) {
    lua_pushstring(L, resource->GetID().filename.c_str()); lua_setfield(L, -2, "texture");
    field(L, "image_width", resource->GetImageWidth()); field(L, "image_height", resource->GetImageHeight());
    field(L, "texture_width", resource->GetTextureWidth()); field(L, "texture_height", resource->GetTextureHeight());
  }
  if (auto* text = dynamic_cast<BitmapText*>(drawing)) {
    lua_pushstring(L, text->GetText().c_str()); lua_setfield(L, -2, "text");
  }
  observe(L, 4);
  lua_settop(L, top); LUA->Release(L);
}
