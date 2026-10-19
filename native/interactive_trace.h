#pragma once
class Actor;
class Screen;
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
  ItgmaniaHarnessNoteScope(int player, int column, float note_beat,
      float draw_beat, int part, bool held, const char* kind);
  ~ItgmaniaHarnessNoteScope();
};

void itgmania_harness_before_screen_update(float delta);
struct ItgmaniaHarnessUpdateFrame {
  explicit ItgmaniaHarnessUpdateFrame(float delta);
  ~ItgmaniaHarnessUpdateFrame();
};
void itgmania_harness_option_delta(const void* options, float delta, bool song);
void itgmania_harness_catalog_inputs(const void* song, float offset, float rate);
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
Actor* itgmania_harness_draw_actor(Actor* actor);
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
