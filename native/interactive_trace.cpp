#include "global.h"
#include "interactive_trace.h"
#include "source_provenance.h"
#include "Actor.h"
#include "ActorMultiVertex.h"
#include "BitmapText.h"
#include "GameInput.h"
#include "GameState.h"
#include "PlayerState.h"
#include "InputFilter.h"
#include "InputMapper.h"
#include "LuaManager.h"
#include "ModelTypes.h"
#include "RageMath.h"
#include "RageDisplay.h"
#include "RageTexture.h"
#include "Screen.h"
#include "ScreenManager.h"
#include "Sprite.h"
#include <chrono>
#include <thread>
#include <map>
#include <mutex>
#include <vector>

namespace {
std::mutex script_errors_mutex;
struct ScriptError { std::string message, screen; };
std::vector<ScriptError> script_errors;
unsigned long long dropped_script_errors = 0;
thread_local std::string loading_screen;
int read_script_errors(lua_State* L) {
  std::lock_guard<std::mutex> lock(script_errors_mutex);
  lua_createtable(L, static_cast<int>(script_errors.size()), 0);
  for (size_t i = 0; i < script_errors.size(); ++i) {
    lua_createtable(L, 0, 2);
    lua_pushlstring(L, script_errors[i].message.data(), script_errors[i].message.size());
    lua_setfield(L, -2, "message");
    lua_pushlstring(L, script_errors[i].screen.data(), script_errors[i].screen.size());
    lua_setfield(L, -2, "screen");
    lua_rawseti(L, -2, static_cast<int>(i + 1));
  }
  lua_pushnumber(L, static_cast<lua_Number>(dropped_script_errors));
  return 2;
}
Actor* drawing = nullptr;
ItgmaniaHarnessActorDrawScope* actor_draw_scope = nullptr;
ItgmaniaHarnessPreDrawScope* predraw_scope = nullptr;
struct RenderRandomInput {
  Actor *owner, *source;
  unsigned long long scope_sequence, draw_ordinal, draw_sequence, update_sequence;
  unsigned int predraw_ordinal;
  int wrapper_index;
  float values[3];
};
std::vector<RenderRandomInput> active_render_random_inputs;
std::map<Actor*, unsigned long long> render_draw_ordinals;
unsigned long long next_render_random_scope = 0;
unsigned long long next_actor_draw_sequence = 0, native_update_sequence = 0;
bool render_random_observer_failed = false;
const msMesh* drawing_model_mesh = nullptr;
bool updating_screen = false;
ItgmaniaHarnessInputScope* input_scope = nullptr;
const ItgmaniaHarnessNoteScope* drawing_note = nullptr;
std::map<const Actor*, float> receptor_alphas;
unsigned long long next_receptor_visit = 0;
unsigned long long next_geometry_draw_sequence = 0;
struct ModelGeometry { int id = 0; std::vector<float> vertices; };
std::map<const msMesh*,ModelGeometry> model_geometries;
int next_model_geometry = 1;
const ItgmaniaHarnessModelGlState* model_gl_state = nullptr;
struct ModelMaterial {
  RageColor emissive, ambient, diffuse, specular;
  float shininess = 0;
  bool observed = false;
} model_material;
int model_texture_modes[4] = {-1, -1, -1, -1};
std::map<uint32_t, std::string> model_texture_names;
int model_render_backend(lua_State* L) {
  if (DISPLAY && DISPLAY->GetApiDescription() == "OpenGL") lua_pushstring(L, "opengl");
  else lua_pushnil(L);
  return 1;
}
bool notefield_only(lua_State* L) {
  lua_getglobal(L,"ITGMANIA_HARNESS_NOTEFIELD_ONLY");
  const bool only=lua_toboolean(L,-1); lua_pop(L,1); return only;
}
bool draw_capture_disabled(lua_State* L) {
  lua_getglobal(L, "ITGMANIA_HARNESS_CAPTURE_FROM_BEAT");
  const bool disabled = lua_isnumber(L, -1) && GAMESTATE
      && GAMESTATE->m_Position.m_fSongBeatVisible < lua_tonumber(L, -1);
  lua_pop(L, 1);
  return disabled;
}
void note_context(lua_State* L) {
  if (!drawing_note) return;
  lua_createtable(L, 0, 7);
  const auto& note = *drawing_note;
  lua_pushinteger(L,note.player); lua_setfield(L,-2,"player");
  lua_pushinteger(L,note.column); lua_setfield(L,-2,"column");
  lua_pushinteger(L,note.part); lua_setfield(L,-2,"part");
  lua_pushnumber(L,note.note_beat); lua_setfield(L,-2,"note_beat");
  lua_pushnumber(L,note.draw_beat); lua_setfield(L,-2,"draw_beat");
  lua_pushboolean(L,note.held); lua_setfield(L,-2,"held");
  lua_pushstring(L,note.kind); lua_setfield(L,-2,"kind");
  if (note.receptor_visit != 0) {
    lua_pushnumber(L,static_cast<lua_Number>(note.receptor_visit));
    lua_setfield(L,-2,"receptor_visit_sequence");
  }
  lua_setfield(L,-2,"note_context");
}
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
void color(lua_State* L, const char* name, const float* values) {
  lua_createtable(L, 4, 0);
  for (int axis = 0; axis < 4; ++axis) { lua_pushnumber(L, values[axis]); lua_rawseti(L, -2, axis + 1); }
  lua_setfield(L, -2, name);
}
void model_render_state(lua_State* L) {
  if (!model_gl_state) return;
  const auto& gl = *model_gl_state;
  lua_createtable(L, 0, 9);
  lua_pushstring(L, "opengl"); lua_setfield(L, -2, "backend");
  boolean(L, "lighting", gl.lighting);
  color(L, "current_color", gl.current_color);
  if (model_material.observed) {
    lua_createtable(L, 0, 5);
    color(L, "emissive", model_material.emissive);
    color(L, "ambient", model_material.ambient);
    color(L, "diffuse", model_material.diffuse);
    color(L, "specular", model_material.specular);
    field(L, "shininess", model_material.shininess);
    lua_setfield(L, -2, "material");
  }
  lua_createtable(L, 0, 7);
  boolean(L, "enabled", gl.blend);
  field(L, "src_rgb", gl.blend_src_rgb); field(L, "dst_rgb", gl.blend_dst_rgb);
  field(L, "src_alpha", gl.blend_src_alpha); field(L, "dst_alpha", gl.blend_dst_alpha);
  field(L, "equation_rgb", gl.blend_equation_rgb); field(L, "equation_alpha", gl.blend_equation_alpha);
  lua_setfield(L, -2, "blend");
  lua_createtable(L, 0, 4);
  boolean(L, "test_enabled", gl.depth_test); boolean(L, "write", gl.depth_write);
  field(L, "function", gl.depth_function);
  lua_createtable(L, 2, 0);
  for (int axis = 0; axis < 2; ++axis) { lua_pushnumber(L, gl.depth_range[axis]); lua_rawseti(L, -2, axis + 1); }
  lua_setfield(L, -2, "range"); lua_setfield(L, -2, "depth");
  lua_createtable(L, 0, 2);
  boolean(L, "enabled", gl.cull); field(L, "face", gl.cull_face);
  lua_setfield(L, -2, "cull");
  lua_createtable(L, 0, 3);
  boolean(L, "enabled", gl.alpha_test); field(L, "function", gl.alpha_function);
  field(L, "reference", gl.alpha_reference); lua_setfield(L, -2, "alpha_test");
  lua_createtable(L, gl.texture_units, 0);
  for (int unit = 0; unit < gl.texture_units; ++unit) {
    const auto& texture = gl.textures[unit];
    lua_createtable(L, 0, 6);
    field(L, "unit", unit); boolean(L, "enabled", texture.enabled);
    field(L, "handle", static_cast<uint32_t>(texture.handle)); field(L, "mode", model_texture_modes[unit]);
    field(L, "environment_mode", texture.environment_mode);
    const auto found = model_texture_names.find(static_cast<uint32_t>(texture.handle));
    if (texture.enabled && found != model_texture_names.end()) {
      lua_pushstring(L, found->second.c_str()); lua_setfield(L, -2, "texture");
    }
    lua_rawseti(L, -2, unit + 1);
  }
  lua_setfield(L, -2, "textures");
  lua_setfield(L, -2, "render_state");
}
void render_random_inputs(lua_State* L) {
  if (render_random_observer_failed) {
    lua_pushstring(L, "native vibrate input observation lost its PreDraw scope");
    lua_setglobal(L, "ITGMANIA_HARNESS_INTERACTIVE_ERROR");
  }
  lua_createtable(L, static_cast<int>(active_render_random_inputs.size()), 0);
  for (size_t i = 0; i < active_render_random_inputs.size(); ++i) {
    const auto& input = active_render_random_inputs[i];
    lua_createtable(L, 0, 7);
    input.owner->PushSelf(L); lua_setfield(L, -2, "actor");
    input.source->PushSelf(L); lua_setfield(L, -2, "source_actor");
    field(L, "native_scope_sequence", static_cast<double>(input.scope_sequence));
    field(L, "draw_ordinal", static_cast<double>(input.draw_ordinal));
    field(L, "native_draw_sequence", static_cast<double>(input.draw_sequence));
    field(L, "native_update_sequence", static_cast<double>(input.update_sequence));
    field(L, "predraw_ordinal", input.predraw_ordinal);
    field(L, "wrapper_index", input.wrapper_index);
    lua_createtable(L, 3, 0);
    for (int axis = 0; axis < 3; ++axis) {
      lua_pushnumber(L, input.values[axis]); lua_rawseti(L, -2, axis + 1);
    }
    lua_setfield(L, -2, "values");
    lua_rawseti(L, -2, static_cast<int>(i + 1));
  }
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
  lua_pushcfunction(L, read_script_errors);
  lua_setglobal(L, "ITGMANIA_HARNESS_SCRIPT_ERRORS");
  lua_newtable(L);
  lua_pushstring(L, HARNESS_NATIVE_REVISION); lua_setfield(L, -2, "revision");
  lua_pushboolean(L, HARNESS_NATIVE_DIRTY); lua_setfield(L, -2, "modified_src");
  lua_pushstring(L, HARNESS_NOTE_DISPLAY_SHA256); lua_setfield(L, -2, "note_display_sha256");
  lua_pushstring(L, HARNESS_RECEPTOR_ARROW_ROW_SHA256); lua_setfield(L, -2, "receptor_arrow_row_sha256");
  lua_pushstring(L, HARNESS_CUBIC_SPLINE_SHA256); lua_setfield(L, -2, "cubic_spline_sha256");
  lua_pushstring(L, HARNESS_ACTOR_SHA256); lua_setfield(L, -2, "actor_sha256");
  lua_pushstring(L, HARNESS_RANDOM_NUMBERS_SHA256); lua_setfield(L, -2, "random_numbers_sha256");
  lua_pushstring(L, HARNESS_RANDOM_NUMBERS_HEADER_SHA256); lua_setfield(L, -2, "random_numbers_header_sha256");
  lua_setglobal(L, "ITGMANIA_HARNESS_NATIVE_SOURCE");
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
  lua_pushboolean(L, true);
  lua_setglobal(L, "ITGMANIA_HARNESS_INPUT_CLOCKS_AVAILABLE");
  lua_pushboolean(L, true);
  lua_setglobal(L, "ITGMANIA_HARNESS_MODEL_GEOMETRY_AVAILABLE");
  lua_pushinteger(L,2); lua_setglobal(L,"ITGMANIA_HARNESS_MODEL_GEOMETRY_PROTOCOL_V2");
  lua_pushinteger(L,1); lua_setglobal(L,"ITGMANIA_HARNESS_MODEL_RENDER_STATE_PROTOCOL_V1");
  lua_pushcfunction(L,model_render_backend); lua_setglobal(L,"ITGMANIA_HARNESS_MODEL_RENDER_STATE_BACKEND");
  lua_pushinteger(L,1); lua_setglobal(L,"ITGMANIA_HARNESS_RECEPTOR_GEOMETRY_PROTOCOL_V1");
  lua_pushinteger(L,1); lua_setglobal(L,"ITGMANIA_HARNESS_RENDER_RANDOM_PROTOCOL");
}
REGISTER_WITH_LUA_FUNCTION(register_interactive)
}

void itgmania_harness_catalog_inputs(const void* song, float offset, float rate) {
  // Raw configuration at note-time calculation, before theme preference changes.
  // Never expose the stored Song getter values through the input bridge.
  catalog_inputs[song] = {offset, rate};
}

void itgmania_harness_script_error(const std::string& message) {
  std::lock_guard<std::mutex> lock(script_errors_mutex);
  // An error storm must not exhaust the recorder; a nonempty sample rejects
  // acceptance regardless of the number of duplicate diagnostics.
  const auto* screen = SCREENMAN ? SCREENMAN->GetTopScreen() : nullptr;
  const std::string name = !loading_screen.empty() ? loading_screen
      : screen ? screen->GetName() : "";
  if (script_errors.size() < 128) script_errors.push_back({message, name});
  else ++dropped_script_errors;
}

ItgmaniaHarnessScreenLoad::ItgmaniaHarnessScreenLoad(const std::string& screen)
    : previous(loading_screen) { loading_screen = screen; }
ItgmaniaHarnessScreenLoad::~ItgmaniaHarnessScreenLoad() { loading_screen = previous; }

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
ItgmaniaHarnessUpdateFrame::ItgmaniaHarnessUpdateFrame(float delta) {
  ++native_update_sequence;
  render_draw_ordinals.clear();
  itgmania_harness_before_screen_update(delta); updating_screen = true;
}
ItgmaniaHarnessUpdateFrame::~ItgmaniaHarnessUpdateFrame() { updating_screen = false; }

void itgmania_harness_after_screen_draw() {
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_AFTER_SCREEN_DRAW");
  if (lua_isfunction(L, -1)) observe(L, 0);
  lua_settop(L, top); LUA->Release(L);
}

ItgmaniaHarnessInputScope::ItgmaniaHarnessInputScope() : previous(input_scope) {
  input_scope = this;
}
ItgmaniaHarnessInputScope::~ItgmaniaHarnessInputScope() { input_scope = previous; }
void itgmania_harness_input_clock(int player, int column, float music_seconds,
    float music_rate, float last_beat_update_ago, float step_ago, float search_seconds) {
  if (input_scope) input_scope->clocks.push_back({player, column, music_seconds,
      music_rate, last_beat_update_ago, step_ago, search_seconds});
}

void itgmania_harness_observe_input(lua_State* L, Screen* screen) {
  if (screen != SCREENMAN->GetTopScreen()) return;
  const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_INPUT_EVENT");
  if (lua_isfunction(L, -1)) {
    lua_pushvalue(L, top);
    lua_newtable(L);
    if (input_scope) for (size_t i = 0; i < input_scope->clocks.size(); ++i) {
      const auto& clock = input_scope->clocks[i];
      lua_createtable(L, 0, 7);
      field(L, "player", clock.player); field(L, "column", clock.column);
      field(L, "music_seconds", clock.music_seconds); field(L, "music_rate", clock.music_rate);
      field(L, "last_beat_update_ago", clock.last_beat_update_ago);
      field(L, "step_ago", clock.step_ago); field(L, "search_seconds", clock.search_seconds);
      lua_rawseti(L, -2, static_cast<int>(i + 1));
    }
    observe(L, 2);
  }
  lua_settop(L, top);
}

Actor* itgmania_harness_draw_actor(Actor* actor) {
  Actor* previous = drawing; drawing = actor; return previous;
}

ItgmaniaHarnessActorDrawScope::ItgmaniaHarnessActorDrawScope(Actor* owner_)
    : previous(actor_draw_scope), owner(owner_),
      active_begin(active_render_random_inputs.size()),
      draw_ordinal(++render_draw_ordinals[owner_]),
      draw_sequence(++next_actor_draw_sequence), update_sequence(native_update_sequence) {
  actor_draw_scope = this;
}
ItgmaniaHarnessActorDrawScope::~ItgmaniaHarnessActorDrawScope() {
  active_render_random_inputs.resize(active_begin);
  actor_draw_scope = previous;
}
ItgmaniaHarnessPreDrawScope::ItgmaniaHarnessPreDrawScope(Actor* owner, Actor* source_, int wrapper_index_)
    : previous(predraw_scope), draw_scope(actor_draw_scope), source(source_),
      wrapper_index(wrapper_index_),
      predraw_ordinal(draw_scope ? ++draw_scope->next_predraw_ordinal : 0) {
  if (!draw_scope || draw_scope->owner != owner) render_random_observer_failed = true;
  predraw_scope = this;
}
ItgmaniaHarnessPreDrawScope::~ItgmaniaHarnessPreDrawScope() {
  predraw_scope = previous;
  if (count == 0) return;
  if (!draw_scope || count != 3) { render_random_observer_failed = true; return; }
  active_render_random_inputs.push_back({draw_scope->owner, source,
      ++next_render_random_scope, draw_scope->draw_ordinal,
      draw_scope->draw_sequence, draw_scope->update_sequence, predraw_ordinal,
      wrapper_index, {values[0], values[1], values[2]}});
}
float itgmania_harness_vibration_random(Actor* source, int axis, float value) {
  // Return the same f32 from the original randomf expression. In particular,
  // zero magnitudes still consume all three native random draws.
  if (!predraw_scope || predraw_scope->source != source || axis < 0 || axis >= 3
      || predraw_scope->count != static_cast<unsigned int>(axis)) {
    render_random_observer_failed = true;
  } else {
    predraw_scope->values[axis] = value;
    ++predraw_scope->count;
  }
  return value;
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
  if (draw_capture_disabled(L) || (notefield_only(L) && !drawing_note)) { LUA->Release(L); return; }
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
  note_context(L);
  field(L, "native_draw_sequence", static_cast<double>(++next_geometry_draw_sequence));
  if (actor_draw_scope) field(L, "actor_draw_sequence", static_cast<double>(actor_draw_scope->draw_sequence));
  field(L, "render_target", static_cast<double>(DISPLAY->GetRenderTarget()));
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
  render_random_inputs(L);
  observe(L, 5);
  lua_settop(L, top); LUA->Release(L);
}

// Observe actual submitted positions, including CPU-skinned vertices and the
// mesh's bone matrix. The OpenGL backend also supplies directly observed draw
// state. Neither geometry nor material state claims final rasterized visibility.
void itgmania_harness_draw_model(const msMesh& mesh, int index,
    const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection) {
  if (!drawing || mesh.Triangles.empty()) return;
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  if (notefield_only(L) && !drawing_note) { LUA->Release(L); return; }
  lua_getglobal(L,"ITGMANIA_HARNESS_MODEL_MESH");
  if (!lua_isfunction(L,-1)) { lua_settop(L,top); LUA->Release(L); return; }
  // Cache by submitted positions, not the source asset: CPU skinning can change
  // the same mesh between draws. Static Cel meshes cross the Lua boundary once.
  std::vector<float> vertices;
  vertices.reserve(mesh.Triangles.size()*9);
  for (const auto& triangle:mesh.Triangles) for (const auto vertex_index:triangle.nVertexIndices) {
    const auto& p=mesh.Vertices.at(vertex_index).p;
    vertices.insert(vertices.end(),{p.x,p.y,p.z});
  }
  auto& geometry=model_geometries[&mesh];
  if (geometry.id==0 || geometry.vertices!=vertices) {
    geometry.id=next_model_geometry++; geometry.vertices=std::move(vertices);
    lua_pushinteger(L,geometry.id);
    lua_createtable(L,static_cast<int>(geometry.vertices.size()/3),0);
    for (size_t i=0;i<geometry.vertices.size();i+=3) {
      lua_createtable(L,3,0);
      for (int k=0;k<3;++k) { lua_pushnumber(L,geometry.vertices[i+k]); lua_rawseti(L,-2,k+1); }
      lua_rawseti(L,-2,static_cast<int>(i/3+1));
    }
    observe(L,2);
  }
  lua_settop(L,top);
  if (draw_capture_disabled(L)) { LUA->Release(L); return; }
  lua_getglobal(L,"ITGMANIA_HARNESS_DRAW_MODEL");
  if (!lua_isfunction(L,-1)) { lua_settop(L,top); LUA->Release(L); return; }
  drawing->PushSelf(L); lua_pushinteger(L,geometry.id);
  lua_createtable(L,0,5);
  note_context(L); field(L,"mesh_index",index);
  field(L,"material_index",mesh.nMaterialIndex);
  model_render_state(L);
  field(L,"native_draw_sequence",static_cast<double>(++next_geometry_draw_sequence));
  if (actor_draw_scope) field(L,"actor_draw_sequence",static_cast<double>(actor_draw_scope->draw_sequence));
  field(L,"render_target",static_cast<double>(DISPLAY->GetRenderTarget()));
  lua_pushstring(L,updating_screen?"during_update":"screen_draw"); lua_setfield(L,-2,"draw_phase");
  lua_pushstring(L,mesh.sName.c_str()); lua_setfield(L,-2,"mesh_name");
  lua_createtable(L,4,0);
  for (int point=0;point<4;++point) {
    RageVector4 local(point==1?1.0f:0.0f,point==2?1.0f:0.0f,point==3?1.0f:0.0f,1.0f),w,v,clip;
    RageVec4TransformCoord(&w,&local,&world); RageVec4TransformCoord(&v,&w,&view);
    RageVec4TransformCoord(&clip,&v,&projection);
    const double values[]={clip.x,clip.y,clip.z,clip.w};
    lua_createtable(L,4,0);
    for(int k=0;k<4;++k){lua_pushnumber(L,values[k]);lua_rawseti(L,-2,k+1);}
    lua_rawseti(L,-2,point+1);
  }
  lua_setfield(L,-2,"anchors_clip");
  observe(L,3); lua_settop(L,top); LUA->Release(L);
}

ItgmaniaHarnessModelMeshScope::ItgmaniaHarnessModelMeshScope(const msMesh* mesh)
    : previous(drawing_model_mesh) { drawing_model_mesh = mesh; }
ItgmaniaHarnessModelMeshScope::~ItgmaniaHarnessModelMeshScope() { drawing_model_mesh = previous; }
void itgmania_harness_draw_model_current(int index,
    const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection) {
  // OpenGL records later, inside its backend, with the active renderer state.
  if (DISPLAY->GetApiDescription() == "OpenGL") return;
  if (drawing_model_mesh) itgmania_harness_draw_model(*drawing_model_mesh, index, world, view, projection);
}

bool itgmania_harness_model_capture_enabled() {
  if (!drawing || !drawing_model_mesh) return false;
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  const bool disabled = draw_capture_disabled(L) || (notefield_only(L) && !drawing_note);
  lua_getglobal(L, "ITGMANIA_HARNESS_DRAW_MODEL");
  const bool enabled = !disabled && lua_isfunction(L, -1);
  lua_settop(L, top); LUA->Release(L); return enabled;
}
void itgmania_harness_model_material(const RageColor& emissive, const RageColor& ambient,
    const RageColor& diffuse, const RageColor& specular, float shininess) {
  model_material = {emissive, ambient, diffuse, specular, shininess, true};
}
void itgmania_harness_model_texture_mode(int unit, int mode) {
  if (unit >= 0 && unit < 4) model_texture_modes[unit] = mode;
}
uintptr_t itgmania_harness_model_texture_handle(RageTexture* texture) {
  const auto handle = texture->GetTexHandle();
  // RageDisplay_Legacy::SetTexture casts the opaque uintptr_t to GLuint.
  // Keep that exact GL-name conversion for identity, without changing the
  // original argument delivered to the native renderer.
  model_texture_names[static_cast<uint32_t>(handle)] = texture->GetID().filename;
  return handle;
}
void itgmania_harness_draw_model_gl(const ItgmaniaHarnessModelGlState& render, int index,
    const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection) {
  const auto* previous = model_gl_state;
  model_gl_state = &render;
  if (drawing_model_mesh) itgmania_harness_draw_model(*drawing_model_mesh, index, world, view, projection);
  model_gl_state = previous;
}

ItgmaniaHarnessNoteScope::ItgmaniaHarnessNoteScope(int player_, int column_,
    float note_beat_, float draw_beat_, int part_, bool held_, const char* kind_)
    : previous(drawing_note), player(player_), column(column_), part(part_),
      note_beat(note_beat_), draw_beat(draw_beat_), held(held_), kind(kind_) { drawing_note = this; }
ItgmaniaHarnessNoteScope::~ItgmaniaHarnessNoteScope() { drawing_note = previous; }

void itgmania_harness_receptor_alpha(const Actor* actor, float alpha) {
  receptor_alphas[actor] = alpha;
}

ItgmaniaHarnessReceptorScope::ItgmaniaHarnessReceptorScope(
    Actor* actor, int player, int column, float beat)
    : note(player, column, beat, beat, -1, false, "receptor") {
  note.receptor_visit = ++next_receptor_visit;
  Lua* L = LUA->Get(); const int top = lua_gettop(L);
  if (draw_capture_disabled(L)) { LUA->Release(L); return; }
  lua_getglobal(L, "ITGMANIA_HARNESS_RECEPTOR_VISIT");
  if (!lua_isfunction(L, -1)) { lua_settop(L, top); LUA->Release(L); return; }
  actor->PushSelf(L);
  lua_createtable(L, 0, 7);
  note_context(L);
  const auto alpha = receptor_alphas.find(actor);
  if (alpha != receptor_alphas.end()) field(L, "base_alpha", alpha->second);
  boolean(L, "visible", actor->GetVisible());
  field(L, "render_target", static_cast<double>(DISPLAY->GetRenderTarget()));
  lua_pushstring(L, updating_screen ? "during_update" : "screen_draw");
  lua_setfield(L, -2, "draw_phase");
  observe(L, 2);
  lua_settop(L, top); LUA->Release(L);
}
