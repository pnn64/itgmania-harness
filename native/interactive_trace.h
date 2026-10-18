#pragma once
class Actor;
class Screen;
struct RageMatrix;
struct RageSpriteVertex;
struct lua_State;

void itgmania_harness_before_screen_update(float delta);
void itgmania_harness_option_delta(const void* options, float delta, bool song);
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
