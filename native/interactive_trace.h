#pragma once
#include <cstddef>
#include <string>
#include <vector>
class Actor;
class Screen;
class InputEventPlus;
struct RageMatrix;
struct RageSpriteVertex;
struct lua_State;
struct msMesh;
struct ItgmaniaHarnessNoteScope {
  const ItgmaniaHarnessNoteScope* previous;
  int player, column, part;
  float note_beat, draw_beat;
  bool held;
  const char* kind;
  unsigned long long receptor_visit = 0;
  ItgmaniaHarnessNoteScope(int player, int column, float note_beat,
      float draw_beat, int part, bool held, const char* kind);
  ~ItgmaniaHarnessNoteScope();
};

// Observe the exact SetBaseAlpha assignment and each attempted receptor draw.
// A transparent actor can submit no geometry, so geometry alone cannot prove
// that receptors were visited by the native renderer.
void itgmania_harness_receptor_alpha(const Actor* actor, float alpha);
struct ItgmaniaHarnessReceptorScope {
  ItgmaniaHarnessNoteScope note;
  ItgmaniaHarnessReceptorScope(Actor* actor, int player, int column, float beat);
};

void itgmania_harness_before_screen_update(float delta);
struct ItgmaniaHarnessUpdateFrame {
  explicit ItgmaniaHarnessUpdateFrame(float delta);
  ~ItgmaniaHarnessUpdateFrame();
};
void itgmania_harness_option_delta(const void* options, float delta, bool song);
void itgmania_harness_catalog_inputs(const void* song, float offset, float rate);
void itgmania_harness_script_error(const std::string& message);
struct ItgmaniaHarnessScreenLoad {
  std::string previous;
  explicit ItgmaniaHarnessScreenLoad(const std::string& screen);
  ~ItgmaniaHarnessScreenLoad();
};
double itgmania_harness_arrow_clock(double value);
void itgmania_harness_arrow_update_clock(double value);
void itgmania_harness_after_arrow_update();
struct ItgmaniaHarnessArrowCacheScope {
  int previous;
  ItgmaniaHarnessArrowCacheScope(int player, bool tangent);
  ~ItgmaniaHarnessArrowCacheScope();
};
void itgmania_harness_after_screen_draw();
struct ItgmaniaHarnessDrawFrame {
  ~ItgmaniaHarnessDrawFrame() { itgmania_harness_after_screen_draw(); }
};
void itgmania_harness_observe_input(lua_State* L, Screen* screen);
struct ItgmaniaHarnessInputClock {
  int player, column;
  float music_seconds, music_rate, last_beat_update_ago, step_ago, search_seconds;
};
struct ItgmaniaHarnessInputScope {
  ItgmaniaHarnessInputScope* previous;
  std::vector<ItgmaniaHarnessInputClock> clocks;
  ItgmaniaHarnessInputScope();
  ~ItgmaniaHarnessInputScope();
};
void itgmania_harness_input_clock(int player, int column, float music_seconds,
    float music_rate, float last_beat_update_ago, float step_ago, float search_seconds);
void itgmania_harness_observe_screen_input(const InputEventPlus& input, Screen* screen);
Actor* itgmania_harness_draw_actor(Actor* actor);
// These scopes retain raw inputs and their native call occurrences only. They
// neither consume randomness nor supply any transform to the engine.
struct ItgmaniaHarnessActorDrawScope {
  ItgmaniaHarnessActorDrawScope* previous;
  Actor* owner;
  std::size_t active_begin;
  unsigned long long draw_ordinal, draw_sequence, update_sequence;
  unsigned int next_predraw_ordinal = 0;
  explicit ItgmaniaHarnessActorDrawScope(Actor* owner);
  ~ItgmaniaHarnessActorDrawScope();
};
struct ItgmaniaHarnessPreDrawScope {
  ItgmaniaHarnessPreDrawScope* previous;
  ItgmaniaHarnessActorDrawScope* draw_scope;
  Actor* source;
  int wrapper_index;
  unsigned int predraw_ordinal, count = 0;
  float values[3] = {};
  ItgmaniaHarnessPreDrawScope(Actor* owner, Actor* source, int wrapper_index);
  ~ItgmaniaHarnessPreDrawScope();
};
float itgmania_harness_vibration_random(Actor* source, int axis, float value);
void itgmania_harness_render_state(Actor* actor, bool write, int test, bool wrap,
    bool filter, int blend, float bias, int cull);
void itgmania_harness_draw_vertices(const char* primitive, const RageSpriteVertex* vertices,
    int count, const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection,
    const RageMatrix& centering, const RageMatrix& texture);
void itgmania_harness_draw_model(const msMesh& mesh, int index,
    const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection);
struct ItgmaniaHarnessModelMeshScope {
  const msMesh* previous;
  explicit ItgmaniaHarnessModelMeshScope(const msMesh* mesh);
  ~ItgmaniaHarnessModelMeshScope();
};
void itgmania_harness_draw_model_current(int index,
    const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection);
