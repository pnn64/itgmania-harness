#include "oracle_bridge.h"
#include "runtime_stubs.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <memory>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

#include "ActorFrame.h"
#include "ActorFrameTexture.h"
#include "ActorProxy.h"
#include "BitmapText.h"
#include "CubicSpline.h"
#include "ModelTypes.h"
#include "Model.h"
#include "ModelManager.h"
#include "MessageManager.h"
#include "LuaManager.h"
#include "LuaBinding.h"
#include "RageTextureRenderTarget.h"
#include "RageDisplay.h"
#include "RageMath.h"
#include "RageTexture.h"
#include "RageTextureID.h"
#include "RageTextureManager.h"
#include "RageTypes.h"
#include "RageUtil/RandomNumbers.h"
#include "ScreenDimensions.h"
#include "Sprite.h"
// The AMV metadata adapter invokes native setters in an isolated Lua state.
// Their ignored self return must not export a manager-owned Actor table there.
// Normal native Lua calls retain the original COMMON_RETURN_SELF behavior.
#undef COMMON_RETURN_SELF
#define COMMON_RETURN_SELF \
  if (L == LUA->Get()) p->PushSelf(L); else lua_pushnil(L); \
  return 1;
#include "ActorMultiVertex.cpp"
#include "Model.cpp"
#undef COMMON_RETURN_SELF
#define COMMON_RETURN_SELF p->PushSelf(L); return 1;
#include "Tween.h"
#include "json/json.h"

namespace {

float g_screen_width = 640.0f;
float g_screen_height = 480.0f;

float number(const Json::Value& value, const std::string& path) {
  if (!value.isNumeric()) {
    throw std::runtime_error(path + " must be a number");
  }
  const double result = value.asDouble();
  if (!std::isfinite(result) || result < -std::numeric_limits<float>::max() ||
      result > std::numeric_limits<float>::max()) {
    throw std::runtime_error(path + " must be a finite float");
  }
  return static_cast<float>(result);
}

float field_number(const Json::Value& object, const char* key,
                   float fallback, const std::string& path) {
  return object.isMember(key) ? number(object[key], path + "." + key)
                              : fallback;
}

bool field_bool(const Json::Value& object, const char* key, bool fallback,
                const std::string& path) {
  if (!object.isMember(key)) return fallback;
  if (!object[key].isBool()) {
    throw std::runtime_error(path + "." + key + " must be a boolean");
  }
  return object[key].asBool();
}

std::string field_string(const Json::Value& object, const char* key,
                         const std::string& fallback,
                         const std::string& path) {
  if (!object.isMember(key)) return fallback;
  if (!object[key].isString()) {
    throw std::runtime_error(path + "." + key + " must be a string");
  }
  return object[key].asString();
}

std::vector<float> float_array(const Json::Value& value, Json::ArrayIndex size,
                               const std::string& path) {
  if (!value.isArray() || value.size() != size) {
    throw std::runtime_error(path + " must contain exactly " +
                             std::to_string(size) + " numbers");
  }
  std::vector<float> result;
  result.reserve(size);
  for (Json::ArrayIndex i = 0; i < size; ++i) {
    result.push_back(number(value[i], path + "[" + std::to_string(i) + "]"));
  }
  return result;
}

RageVector3 vec3(const Json::Value& object, const char* key,
                 RageVector3 fallback, const std::string& path) {
  if (!object.isMember(key)) return fallback;
  const auto values = float_array(object[key], 3, path + "." + key);
  return RageVector3(values[0], values[1], values[2]);
}

RageColor color(const Json::Value& object, const char* key,
                RageColor fallback, const std::string& path) {
  if (!object.isMember(key)) return fallback;
  const auto values = float_array(object[key], 4, path + "." + key);
  return RageColor(values[0], values[1], values[2], values[3]);
}

Json::Value vector2_json(const RageVector2& value) {
  Json::Value result(Json::arrayValue);
  result.append(value.x);
  result.append(value.y);
  return result;
}

Json::Value vector3_json(const RageVector3& value) {
  Json::Value result(Json::arrayValue);
  result.append(value.x);
  result.append(value.y);
  result.append(value.z);
  return result;
}

Json::Value vector4_json(const RageVector4& value) {
  Json::Value result(Json::arrayValue);
  result.append(value.x);
  result.append(value.y);
  result.append(value.z);
  result.append(value.w);
  return result;
}

Json::Value color_json(const RageColor& value) {
  Json::Value result(Json::arrayValue);
  result.append(value.r);
  result.append(value.g);
  result.append(value.b);
  result.append(value.a);
  return result;
}

Json::Value rect_json(const RectF& value) {
  Json::Value result(Json::arrayValue);
  result.append(value.left);
  result.append(value.top);
  result.append(value.right);
  result.append(value.bottom);
  return result;
}

Json::Value matrix_json(const RageMatrix& value) {
  Json::Value rows(Json::arrayValue);
  for (int row = 0; row < 4; ++row) {
    Json::Value columns(Json::arrayValue);
    for (int column = 0; column < 4; ++column) {
      columns.append(value(row, column));
    }
    rows.append(std::move(columns));
  }
  return rows;
}

Json::Value state_json(const Actor::TweenState& state) {
  Json::Value result(Json::objectValue);
  result["position"] = vector3_json(state.pos);
  result["rotation"] = vector3_json(state.rotation);
  result["quaternion"] = vector4_json(state.quat);
  result["zoom"] = vector3_json(state.scale);
  result["skew"] = vector2_json(RageVector2(state.fSkewX, state.fSkewY));
  result["crop"] = rect_json(state.crop);
  result["fade"] = rect_json(state.fade);
  Json::Value diffuse(Json::arrayValue);
  for (int i = 0; i < NUM_DIFFUSE_COLORS; ++i) {
    diffuse.append(color_json(state.diffuse[i]));
  }
  result["diffuse"] = std::move(diffuse);
  result["glow"] = color_json(state.glow);
  result["aux"] = state.aux;
  return result;
}

const char* effect_name(Actor::Effect effect) {
  switch (effect) {
    case Actor::no_effect: return "none";
    case Actor::diffuse_blink: return "diffuse_blink";
    case Actor::diffuse_shift: return "diffuse_shift";
    case Actor::diffuse_ramp: return "diffuse_ramp";
    case Actor::glow_blink: return "glow_blink";
    case Actor::glow_shift: return "glow_shift";
    case Actor::glow_ramp: return "glow_ramp";
    case Actor::rainbow: return "rainbow";
    case Actor::wag: return "wag";
    case Actor::bounce: return "bounce";
    case Actor::bob: return "bob";
    case Actor::pulse: return "pulse";
    case Actor::spin: return "spin";
    case Actor::vibrate: return "vibrate";
  }
  return "unknown";
}

const char* clock_name(Actor::EffectClock clock) {
  switch (clock) {
    case Actor::CLOCK_TIMER: return "timer";
    case Actor::CLOCK_TIMER_GLOBAL: return "timer_global";
    case Actor::CLOCK_BGM_TIME: return "bgm_time";
    case Actor::CLOCK_BGM_BEAT: return "bgm_beat";
    case Actor::CLOCK_BGM_TIME_NO_OFFSET: return "bgm_time_no_offset";
    case Actor::CLOCK_BGM_BEAT_NO_OFFSET: return "bgm_beat_no_offset";
    case Actor::CLOCK_BGM_BEAT_PLAYER1: return "bgm_beat_player1";
    case Actor::CLOCK_BGM_BEAT_PLAYER2: return "bgm_beat_player2";
    default: return "light";
  }
}

const char* texture_mode_name(TextureMode mode) {
  switch (mode) {
    case TextureMode_Modulate: return "modulate";
    case TextureMode_Glow: return "glow";
    case TextureMode_Add: return "add";
    default: return "invalid";
  }
}

class HarnessDisplay;

class HarnessGeometry final : public RageCompiledGeometry {
 public:
  explicit HarnessGeometry(HarnessDisplay* display) : display_(display) {}
  void Allocate(const std::vector<msMesh>& meshes) override {
    meshes_.reserve(meshes.size());
  }
  void Change(const std::vector<msMesh>& meshes) override { meshes_ = meshes; }
  void Draw(int index) const override;
 private:
  HarnessDisplay* display_;
  std::vector<msMesh> meshes_;
};

class HarnessDisplay final : public RageDisplay {
 public:
  std::vector<RenderTargetParam> allocations;
  explicit HarnessDisplay(int width, int height)
      : params_(true, "harness", width, height, 32, 60, false, false, false,
                false, false, false, "ITGmania actor harness", "", false,
                static_cast<float>(width) / static_cast<float>(height)),
        width_(width),
        height_(height) {}

  std::string Init(const VideoModeParams&, bool) override { return {}; }
  std::string GetApiDescription() const override { return "HarnessCapture"; }
  void GetDisplaySpecs(DisplaySpecs&) const override {}
  const RagePixelFormatDesc* GetPixelFormatDesc(RagePixelFormat) const override {
    static const RagePixelFormatDesc desc = {32, {0xff000000, 0x00ff0000,
                                                  0x0000ff00, 0x000000ff}};
    return &desc;
  }
  ActualVideoModeParams GetActualVideoModeParams() const override {
    return ActualVideoModeParams(params_);
  }
  void SetBlendMode(BlendMode mode) override { blend_mode_ = mode; }
  bool SupportsTextureFormat(RagePixelFormat, bool) override { return true; }
  bool SupportsPerVertexMatrixScale() override { return false; }
  uintptr_t CreateTexture(RagePixelFormat, RageSurface*, bool) override {
    return 1;
  }
  void UpdateTexture(uintptr_t, RageSurface*, int, int, int, int) override {}
  void DeleteTexture(uintptr_t) override {}
  void ClearAllTextures() override {}
  bool SupportsRenderToTexture() const override { return true; }
  uintptr_t CreateRenderTarget(const RenderTargetParam& param, int& width,
                              int& height) override {
    allocations.push_back(param);
    width = param.iWidth;
    height = param.iHeight;
    const uintptr_t handle = targets_.size() + 2;
    targets_[handle] = {width, height};
    return handle;
  }
  uintptr_t GetRenderTarget() override { return target_; }
  void SetRenderTarget(uintptr_t handle, bool = true) override { target_ = handle; }
  int GetNumTextureUnits() override { return 1; }
  void SetTexture(TextureUnit, uintptr_t) override {}
  void SetTextureMode(TextureUnit, TextureMode mode) override {
    texture_mode_ = mode;
  }
  void SetTextureWrapping(TextureUnit, bool) override {}
  int GetMaxTextureSize() const override { return 4096; }
  void SetTextureFiltering(TextureUnit, bool) override {}
  bool IsZTestEnabled() const override { return z_test_ != ZTEST_OFF; }
  bool IsZWriteEnabled() const override { return z_write_; }
  void SetZWrite(bool value) override { z_write_ = value; }
  void SetZTestMode(ZTestMode mode) override { z_test_ = mode; }
  void SetZBias(float) override {}
  void ClearZBuffer() override {}
  void SetCullMode(CullMode mode) override { cull_mode_ = mode; }
  void SetAlphaTest(bool) override {}
  void SetMaterial(const RageColor& emissive, const RageColor& ambient, const RageColor& diffuse,
                   const RageColor& specular, float shininess) override {
    material_["emissive"] = color_json(emissive);
    material_["ambient"] = color_json(ambient);
    material_["diffuse"] = color_json(diffuse);
    material_["specular"] = color_json(specular);
    material_["shininess"] = shininess;
  }
  void SetLighting(bool enabled) override { lighting_ = enabled; }
  void SetLightOff(int index) override { lights_.removeMember(std::to_string(index)); }
  void SetLightDirectional(int index, const RageColor& ambient, const RageColor& diffuse,
                           const RageColor& specular, const RageVector3& direction) override {
    auto& light = lights_[std::to_string(index)];
    light["ambient"] = color_json(ambient);
    light["diffuse"] = color_json(diffuse);
    light["specular"] = color_json(specular);
    light["direction"] = vector3_json(direction);
  }
  void SetSphereEnvironmentMapping(TextureUnit, bool) override {}
  void SetCelShaded(int) override {}
  RageCompiledGeometry* CreateCompiledGeometry() override { return new HarnessGeometry(this); }
  void DeleteCompiledGeometry(RageCompiledGeometry* geometry) override { delete geometry; }
  RageSurface* CreateScreenshot() override { return nullptr; }

  RageMatrix world() const { return *GetWorldTop(); }
  RageMatrix view() const { return *GetViewTop(); }
  RageMatrix projection() const { return *GetProjectionTop(); }
  RageMatrix centering() const { return *GetCentering(); }
  RageMatrix texture() const { return *GetTextureTop(); }

  void start_sample(Json::Value* actors, Json::Value* draw_sequence) {
    actors_ = actors;
    draw_sequence_ = draw_sequence;
    current_actor_ = -1;
    primitive_index_ = 0;
  }

  void set_actor(int index) {
    current_actor_ = index;
    (*actors_)[index]["drawn"] = true;
    draw_sequence_->append((*actors_)[index]["name"]);
  }

  void clear_actor() { current_actor_ = -1; }

  void capture_mesh(const msMesh& mesh, int index) {
    std::vector<RageSpriteVertex> vertices;
    Json::Value normals(Json::arrayValue), texture_scale(Json::arrayValue);
    vertices.reserve(mesh.Triangles.size() * 3);
    for (const auto& triangle : mesh.Triangles) {
      for (const auto vertex_index : triangle.nVertexIndices) {
        const auto& source = mesh.Vertices.at(vertex_index);
        RageSpriteVertex vertex;
        vertex.p = source.p;
        vertex.t = source.t;
        vertex.c.r = vertex.c.g = vertex.c.b = vertex.c.a = 255;
        vertices.push_back(vertex);
        normals.append(vector3_json(source.n));
        texture_scale.append(vector2_json(source.TextureMatrixScale));
      }
    }
    capture("triangles", vertices.data(), static_cast<int>(vertices.size()));
    if (current_actor_ >= 0 && actors_ != nullptr) {
      auto& draws = (*actors_)[current_actor_]["draws"];
      auto& draw = draws[draws.size() - 1];
      draw["model_mesh_index"] = index;
      draw["model_mesh_name"] = mesh.sName;
      draw["normals"] = std::move(normals);
      draw["texture_matrix_scale"] = std::move(texture_scale);
      draw["texture_matrix"] = matrix_json(texture());
      draw["material"] = material_;
      draw["lighting"] = lighting_;
      draw["lights"] = lights_;
      draw["cull_mode"] = static_cast<int>(cull_mode_);
      draw["z_write"] = z_write_;
      draw["z_test"] = static_cast<int>(z_test_);
    }
  }

 protected:
  void DrawQuadsInternal(const RageSpriteVertex vertices[], int count) override {
    capture("quads", vertices, count);
  }
  void DrawQuadStripInternal(const RageSpriteVertex vertices[], int count) override {
    capture("quad_strip", vertices, count);
  }
  void DrawFanInternal(const RageSpriteVertex vertices[], int count) override {
    capture("fan", vertices, count);
  }
  void DrawStripInternal(const RageSpriteVertex vertices[], int count) override {
    capture("strip", vertices, count);
  }
  void DrawTrianglesInternal(const RageSpriteVertex vertices[], int count) override {
    capture("triangles", vertices, count);
  }
  void DrawCompiledGeometryInternal(const RageCompiledGeometry* geometry, int index) override {
    geometry->Draw(index);
  }
  void DrawLineStripInternal(const RageSpriteVertex vertices[], int count,
                             float) override {
    capture("line_strip", vertices, count);
  }
  void DrawSymmetricQuadStripInternal(const RageSpriteVertex vertices[],
                                      int count) override {
    capture("symmetric_quad_strip", vertices, count);
  }
  std::string TryVideoMode(const VideoModeParams& params, bool& fresh) override {
    params_ = params;
    fresh = false;
    return {};
  }
  RageMatrix GetOrthoMatrix(float left, float right, float bottom, float top,
                            float near_z, float far_z) override {
    return RageMatrix(
        2 / (right - left), 0, 0, 0, 0, 2 / (top - bottom), 0, 0, 0, 0,
        -2 / (far_z - near_z), 0, -(right + left) / (right - left),
        -(top + bottom) / (top - bottom), -(far_z + near_z) / (far_z - near_z),
        1);
  }

 private:
  static Json::Value vcolor_json(const RageVColor& color) {
    Json::Value result(Json::arrayValue);
    result.append(color.r);
    result.append(color.g);
    result.append(color.b);
    result.append(color.a);
    return result;
  }

  void capture(const char* primitive, const RageSpriteVertex vertices[],
               int count) {
    if (current_actor_ < 0 || actors_ == nullptr) return;
    const RageMatrix world_matrix = world();
    const RageMatrix view_matrix = view();
    const RageMatrix projection_matrix = projection();
    const RageMatrix centering_matrix = centering();
    const RageMatrix texture_matrix = texture();
    Json::Value draw(Json::objectValue);
    draw["primitive"] = primitive;
    draw["primitive_index"] = primitive_index_++;
    draw["texture_mode"] = texture_mode_name(texture_mode_);
    draw["blend_mode"] = static_cast<int>(blend_mode_);
    const auto viewport = target_ ? targets_.at(target_) : std::pair<int, int>{width_, height_};
    draw["render_target"] = Json::UInt64(target_);
    draw["viewport"] = vector2_json(RageVector2(viewport.first, viewport.second));
    Json::Value output_vertices(Json::arrayValue);
    for (int i = 0; i < count; ++i) {
      RageVector4 local(vertices[i].p.x, vertices[i].p.y, vertices[i].p.z, 1);
      RageVector4 world_pos;
      RageVector4 view_pos;
      RageVector4 clip_pos;
      RageVector4 centered_pos;
      RageVec4TransformCoord(&world_pos, &local, &world_matrix);
      RageVec4TransformCoord(&view_pos, &world_pos, &view_matrix);
      RageVec4TransformCoord(&clip_pos, &view_pos, &projection_matrix);
      RageVec4TransformCoord(&centered_pos, &clip_pos, &centering_matrix);
      RageVector4 local_uv(vertices[i].t.x, vertices[i].t.y, 0, 1);
      RageVector4 transformed_uv;
      RageVec4TransformCoord(&transformed_uv, &local_uv, &texture_matrix);
      Json::Value vertex(Json::objectValue);
      vertex["local"] = vector4_json(local);
      vertex["world"] = vector4_json(world_pos);
      vertex["view"] = vector4_json(view_pos);
      vertex["clip"] = vector4_json(centered_pos);
      Json::Value ndc(Json::arrayValue);
      const float inverse_w = centered_pos.w == 0 ? 0 : 1.0f / centered_pos.w;
      ndc.append(centered_pos.x * inverse_w);
      ndc.append(centered_pos.y * inverse_w);
      ndc.append(centered_pos.z * inverse_w);
      vertex["ndc"] = ndc;
      Json::Value screen(Json::arrayValue);
      screen.append((centered_pos.x * inverse_w + 1) * viewport.first * 0.5f);
      screen.append((1 - centered_pos.y * inverse_w) * viewport.second * 0.5f);
      screen.append(centered_pos.z * inverse_w);
      vertex["screen"] = std::move(screen);
      vertex["uv"] = vector2_json(vertices[i].t);
      vertex["transformed_uv"] =
          vector2_json(RageVector2(transformed_uv.x, transformed_uv.y));
      vertex["color"] = vcolor_json(vertices[i].c);
      output_vertices.append(std::move(vertex));
    }
    draw["vertices"] = std::move(output_vertices);
    (*actors_)[current_actor_]["draws"].append(std::move(draw));
  }

  VideoModeParams params_;
  int width_;
  int height_;
  uintptr_t target_ = 0;
  std::map<uintptr_t, std::pair<int, int>> targets_;
  Json::Value* actors_ = nullptr;
  Json::Value* draw_sequence_ = nullptr;
  int current_actor_ = -1;
  int primitive_index_ = 0;
  TextureMode texture_mode_ = TextureMode_Modulate;
  BlendMode blend_mode_ = BLEND_NORMAL;
  bool z_write_ = false;
  ZTestMode z_test_ = ZTEST_OFF;
  Json::Value material_{Json::objectValue};
  Json::Value lights_{Json::objectValue};
  CullMode cull_mode_ = CULL_NONE;
  bool lighting_ = false;
};

void HarnessGeometry::Draw(int index) const { display_->capture_mesh(meshes_.at(index), index); }

struct HarnessModelScope {
  ModelManager manager;
  ModelManager* previous = MODELMAN;
  HarnessModelScope() {
    MODELMAN = &manager;
    ModelManagerPrefs prefs;
    prefs.m_bDelayedUnload = false;
    manager.SetPrefs(prefs);
  }
  ~HarnessModelScope() { MODELMAN = previous; }
};

void apply_state(Actor& actor, const Json::Value& state,
                 const std::string& path);
void apply_effect(Actor& actor, const Json::Value& effect,
                  const std::string& path);
void apply_tweens(Actor& actor, const Json::Value& tweens,
                  const std::string& path);

template <class Base>
class HarnessActorAccess : public Base {
 public:
  void bind_commands(const Json::Value& commands, const std::string& path) {
    if (commands.isNull()) return;
    if (!commands.isObject()) throw std::runtime_error(path + " must be an object");
    for (const auto& name : commands.getMemberNames()) {
      const auto& command = commands[name];
      if (!command.isBool()) {
        if (!command.isObject() ||
            (command.getMemberNames() != std::vector<std::string>{"size"} &&
             command.getMemberNames() != std::vector<std::string>{"effect"} &&
             command.getMemberNames() != std::vector<std::string>{"tweens"} &&
             command.getMemberNames() != std::vector<std::string>{"state"} &&
             command.getMemberNames() != std::vector<std::string>{"state", "tweens"} &&
             command.getMemberNames() != std::vector<std::string>{"child", "state"} &&
             command.getMemberNames() != std::vector<std::string>{"control"}))
          throw std::runtime_error(path + "." + name + " must be a visibility boolean, size, state, effect, tween or control command");
        if (command.isMember("size"))
          float_array(command["size"], 2, path + "." + name + ".size");
        if (command.isMember("control") && command["control"] != "stop")
          throw std::runtime_error(path + "." + name + ".control must be stop");
      }
    }
    commands_ = commands;
  }

  void HandleMessage(const Message& message) override {
    // Fixture commands change visibility, size, effects or queues; Actor::QueueCommand and
    // UpdateTweening decide when these bodies run, including zero-delta updates.
    if (commands_.isMember(message.GetName())) {
      const auto& command = commands_[message.GetName()];
      if (command.isBool()) this->SetVisible(command.asBool());
      else {
        if (command.isMember("control")) this->StopTweening();
        if (command.isMember("state")) {
          Actor* target = this;
          if (command.isMember("child")) {
            auto* frame = dynamic_cast<ActorFrame*>(this);
            if (!frame || !command["child"].isString())
              throw std::runtime_error("child state command requires an ActorFrame and child name");
            target = frame->GetChild(command["child"].asString());
            if (!target) throw std::runtime_error("child state command has no matching actor");
          }
          apply_state(*target, command["state"], "command." + message.GetName());
        }
        if (command.isMember("effect"))
          apply_effect(*this, command["effect"], "command." + message.GetName());
        if (command.isMember("tweens"))
          apply_tweens(*this, command["tweens"], "command." + message.GetName());
        if (command.isMember("size")) {
          this->SetWidth(command["size"][0].asFloat());
          this->SetHeight(command["size"][1].asFloat());
        }
      }
      if (message.IsBroadcast()) broadcast_commands_.append(message.GetName());
      played_commands_.append(message.GetName());
    } else {
      Base::HandleMessage(message);
    }
  }

  void bind_capture(HarnessDisplay* display, int index) {
    capture_display_ = display;
    capture_index_ = index;
  }

  Json::Value snapshot(const char* kind) const {
    Json::Value result(Json::objectValue);
    result["name"] = this->m_sName;
    result["kind"] = kind;
    result["drawn"] = false;
    result["draw_order"] = this->m_iDrawOrder;
    result["visible"] = this->GetVisible();
    result["size"] = vector2_json(this->m_size);
    result["alignment"] = vector2_json(
        RageVector2(this->m_fHorizAlign, this->m_fVertAlign));
    result["base_rotation"] = vector3_json(this->m_baseRotation);
    result["base_zoom"] = vector3_json(this->m_baseScale);
    result["base_alpha"] = this->m_fBaseAlpha;
    result["current"] = state_json(this->m_current);
    result["start"] = state_json(this->m_start);
    result["destination"] = state_json(this->DestTweenState());
    result["tween_time_left"] = this->GetTweenTimeLeft();
    Json::Value queue(Json::arrayValue);
    for (const auto* queued : this->m_Tweens) {
      Json::Value item(Json::objectValue);
      item["duration"] = queued->info.m_fTweenTime;
      item["time_left"] = queued->info.m_fTimeLeftInTween;
      item["command"] = queued->info.m_sCommandName;
      Json::Value probes(Json::arrayValue);
      for (float at : {0.0f, 0.25f, 0.5f, 0.75f, 1.0f}) {
        probes.append(queued->info.m_pTween->Tween(at));
      }
      item["curve_probe"] = std::move(probes);
      item["target"] = state_json(queued->state);
      queue.append(std::move(item));
    }
    result["tween_queue"] = std::move(queue);
    if (!commands_.isNull()) result["played_commands"] = played_commands_;
    if (!broadcast_commands_.empty()) result["broadcast_commands"] = broadcast_commands_;
    Json::Value effect(Json::objectValue);
    effect["type"] = effect_name(this->m_Effect);
    effect["clock"] = clock_name(this->m_EffectClock);
    effect["seconds"] = this->m_fSecsIntoEffect;
    effect["delta"] = this->m_fEffectDelta;
    effect["period"] = this->m_effect_period;
    effect["offset"] = this->m_fEffectOffset;
    effect["magnitude"] = vector3_json(this->m_vEffectMagnitude);
    effect["color1"] = color_json(this->m_effectColor1);
    effect["color2"] = color_json(this->m_effectColor2);
    result["effect"] = std::move(effect);
    result["draws"] = Json::Value(Json::arrayValue);
    return result;
  }

 protected:
  void capture_transform(const Actor::TweenState& effected) {
    const RageMatrix parent = capture_display_->world();
    DISPLAY->PushMatrix();
    DISPLAY->LoadIdentity();
    Actor::BeginDraw();
    const RageMatrix local = capture_display_->world();
    Actor::EndDraw();
    DISPLAY->PopMatrix();
    if constexpr (std::is_base_of_v<ActorFrame, Base>) {
      ActorFrame::BeginDraw();
    } else {
      Actor::BeginDraw();
    }
    Json::Value& record = (*sample_actors_)[capture_index_];
    record["effected"] = state_json(effected);
    record["parent_matrix"] = matrix_json(parent);
    record["local_matrix"] = matrix_json(local);
    record["world_matrix"] = matrix_json(capture_display_->world());
    record["view_matrix"] = matrix_json(capture_display_->view());
    record["projection_matrix"] = matrix_json(capture_display_->projection());
  }

 public:
  void bind_sample(Json::Value* actors) { sample_actors_ = actors; }
  int capture_index() const { return capture_index_; }

 private:
  Json::Value commands_;
  Json::Value played_commands_{Json::arrayValue};
  Json::Value broadcast_commands_{Json::arrayValue};
  HarnessDisplay* capture_display_ = nullptr;
  Json::Value* sample_actors_ = nullptr;
  int capture_index_ = -1;
};

class HarnessSprite final : public HarnessActorAccess<Sprite> {
 public:
  void BeginDraw() override { capture_transform(*m_pTempState); }
  void DrawPrimitives() override {
    capture_display()->set_actor(capture_index());
    Sprite::DrawPrimitives();
    capture_display()->clear_actor();
  }
  HarnessDisplay* capture_display() { return display_; }
  void set_display(HarnessDisplay* display) { display_ = display; }

 private:
  HarnessDisplay* display_ = nullptr;
};

class HarnessFrame final : public HarnessActorAccess<ActorFrame> {
 public:
  void BeginDraw() override { capture_transform(*m_pTempState); }
};

class HarnessModel final : public HarnessActorAccess<Model> {
 public:
  void BeginDraw() override { capture_transform(*m_pTempState); }
  void DrawPrimitives() override {
    display_->set_actor(capture_index());
    Model::DrawPrimitives();
    display_->clear_actor();
  }
  void set_display(HarnessDisplay* display) { display_ = display; }
 private:
  HarnessDisplay* display_ = nullptr;
};

class HarnessTextureFrame final : public HarnessActorAccess<ActorFrameTexture> {
 public:
  void BeginDraw() override { capture_transform(*m_pTempState); }
};

Actor::EffectClock parse_clock(const std::string& name,
                               const std::string& path) {
  if (name == "timer") return Actor::CLOCK_TIMER;
  if (name == "bgm_time") return Actor::CLOCK_BGM_TIME;
  if (name == "bgm_beat") return Actor::CLOCK_BGM_BEAT;
  if (name == "bgm_time_no_offset") return Actor::CLOCK_BGM_TIME_NO_OFFSET;
  if (name == "bgm_beat_no_offset") return Actor::CLOCK_BGM_BEAT_NO_OFFSET;
  if (name == "bgm_beat_player1") return Actor::CLOCK_BGM_BEAT_PLAYER1;
  if (name == "bgm_beat_player2") return Actor::CLOCK_BGM_BEAT_PLAYER2;
  throw std::runtime_error(path + ".clock has unknown value `" + name + "`");
}

TweenType parse_curve(const std::string& name, const std::string& path) {
  if (name == "linear") return TWEEN_LINEAR;
  if (name == "accelerate") return TWEEN_ACCELERATE;
  if (name == "decelerate") return TWEEN_DECELERATE;
  if (name == "spring") return TWEEN_SPRING;
  throw std::runtime_error(path + ".curve has unknown value `" + name + "`");
}

ITween* make_bezier(const Json::Value& tween, const std::string& path) {
  const Json::Value& control = tween["bezier"];
  if (!control.isArray() || (control.size() != 4 && control.size() != 8)) {
    throw std::runtime_error(path + ".bezier must contain 4 or 8 numbers");
  }
  LUA->RegisterTypes();
  Lua* state = LUA->Get();
  lua_pushinteger(state, TWEEN_BEZIER);
  lua_newtable(state);
  for (Json::ArrayIndex i = 0; i < control.size(); ++i) {
    lua_pushnumber(state, number(control[i], path + ".bezier[" +
                                           std::to_string(i) + "]"));
    lua_rawseti(state, -2, i + 1);
  }
  const int type_position = lua_gettop(state) - 1;
  ITween* result = ITween::CreateFromStack(state, type_position);
  lua_pop(state, 2);
  LUA->Release(state);
  if (result == nullptr) throw std::runtime_error(path + ".bezier was rejected");
  return result;
}

void apply_state(Actor& actor, const Json::Value& state,
                 const std::string& path) {
  if (!state.isObject()) throw std::runtime_error(path + " must be an object");
  if (state.isMember("position")) {
    const auto v = float_array(state["position"], 3, path + ".position");
    actor.SetX(v[0]); actor.SetY(v[1]); actor.SetZ(v[2]);
  }
  if (state.isMember("rotation")) {
    const auto v = float_array(state["rotation"], 3, path + ".rotation");
    actor.SetRotationX(v[0]); actor.SetRotationY(v[1]); actor.SetRotationZ(v[2]);
  }
  if (state.isMember("add_rotation_hpr")) {
    const auto v = float_array(state["add_rotation_hpr"], 3,
                               path + ".add_rotation_hpr");
    actor.AddRotationH(v[0]); actor.AddRotationP(v[1]); actor.AddRotationR(v[2]);
  }
  if (state.isMember("zoom")) {
    const auto v = float_array(state["zoom"], 3, path + ".zoom");
    actor.SetZoomX(v[0]); actor.SetZoomY(v[1]); actor.SetZoomZ(v[2]);
  }
  if (state.isMember("zoom_to_width"))
    actor.ZoomToWidth(number(state["zoom_to_width"], path + ".zoom_to_width"));
  if (state.isMember("zoom_to_height"))
    actor.ZoomToHeight(number(state["zoom_to_height"], path + ".zoom_to_height"));
  if (state.isMember("skew")) {
    const auto v = float_array(state["skew"], 2, path + ".skew");
    actor.SetSkewX(v[0]); actor.SetSkewY(v[1]);
  }
  auto apply_rect = [&](const char* key, auto setter) {
    if (!state.isMember(key)) return;
    const auto v = float_array(state[key], 4, path + "." + key);
    setter(v);
  };
  apply_rect("crop", [&](const std::vector<float>& v) {
    actor.SetCropLeft(v[0]); actor.SetCropTop(v[1]);
    actor.SetCropRight(v[2]); actor.SetCropBottom(v[3]);
  });
  apply_rect("fade", [&](const std::vector<float>& v) {
    actor.SetFadeLeft(v[0]); actor.SetFadeTop(v[1]);
    actor.SetFadeRight(v[2]); actor.SetFadeBottom(v[3]);
  });
  if (state.isMember("diffuse")) {
    const auto v = float_array(state["diffuse"], 4, path + ".diffuse");
    actor.SetDiffuse(RageColor(v[0], v[1], v[2], v[3]));
  }
  if (state.isMember("diffuse_corners")) {
    const Json::Value& corners = state["diffuse_corners"];
    if (!corners.isArray() || corners.size() != 4) {
      throw std::runtime_error(path + ".diffuse_corners must contain 4 colors");
    }
    for (Json::ArrayIndex i = 0; i < 4; ++i) {
      const auto v = float_array(corners[i], 4, path + ".diffuse_corners[" +
                                                std::to_string(i) + "]");
      actor.SetDiffuses(i, RageColor(v[0], v[1], v[2], v[3]));
    }
  }
  if (state.isMember("glow")) {
    const auto v = float_array(state["glow"], 4, path + ".glow");
    actor.SetGlow(RageColor(v[0], v[1], v[2], v[3]));
  }
  if (state.isMember("aux")) actor.SetAux(number(state["aux"], path + ".aux"));
}

void apply_common(Actor& actor, const Json::Value& spec,
                  const std::string& path) {
  actor.SetName(field_string(spec, "name", "", path));
  if (!spec["name"].isString()) throw std::runtime_error(path + ".name is required");
  actor.SetDrawOrder(static_cast<int>(field_number(spec, "draw_order", 0, path)));
  if (spec.isMember("visible")) actor.SetVisible(field_bool(spec, "visible", true, path));
  if (spec.isMember("size")) {
    const auto size = float_array(spec["size"], 2, path + ".size");
    actor.SetWidth(size[0]); actor.SetHeight(size[1]);
  }
  if (spec.isMember("alignment")) {
    const auto alignment = float_array(spec["alignment"], 2, path + ".alignment");
    actor.SetHorizAlign(alignment[0]); actor.SetVertAlign(alignment[1]);
  }
  const Json::Value& base = spec["base"];
  if (!base.isNull()) {
    if (!base.isObject()) throw std::runtime_error(path + ".base must be an object");
    const RageVector3 rotation = vec3(base, "rotation", RageVector3(0, 0, 0),
                                      path + ".base");
    const RageVector3 zoom = vec3(base, "zoom", RageVector3(1, 1, 1),
                                  path + ".base");
    actor.SetBaseRotation(rotation);
    actor.SetBaseZoomX(zoom.x); actor.SetBaseZoomY(zoom.y); actor.SetBaseZoomZ(zoom.z);
    actor.SetBaseAlpha(field_number(base, "alpha", 1, path + ".base"));
  }
  if (spec.isMember("texture_translate")) {
    const auto v = float_array(spec["texture_translate"], 2,
                               path + ".texture_translate");
    actor.SetTextureTranslate(v[0], v[1]);
  }
  if (spec.isMember("state")) apply_state(actor, spec["state"], path + ".state");
}

void apply_effect(Actor& actor, const Json::Value& effect,
                  const std::string& path) {
  if (effect.isNull()) return;
  if (!effect.isObject()) throw std::runtime_error(path + " must be an object");
  const std::string type = field_string(effect, "type", "none", path);
  const float period = field_number(effect, "period", 1, path);
  if (period <= 0) throw std::runtime_error(path + ".period must be positive");
  const RageVector3 magnitude = vec3(effect, "magnitude",
      type == "vibrate" ? RageVector3(10, 10, 10) : RageVector3(0, 0, 10), path);
  const RageColor color1 = color(effect, "color1", RageColor(1, 1, 1, 1), path);
  const RageColor color2 = color(effect, "color2", RageColor(1, 1, 1, 1), path);
  if (type == "none") actor.StopEffect();
  else if (type == "diffuse_blink") actor.SetEffectDiffuseBlink(period, color1, color2);
  else if (type == "diffuse_shift") actor.SetEffectDiffuseShift(period, color1, color2);
  else if (type == "diffuse_ramp") actor.SetEffectDiffuseRamp(period, color1, color2);
  else if (type == "glow_blink") actor.SetEffectGlowBlink(period, color1, color2);
  else if (type == "glow_shift") actor.SetEffectGlowShift(period, color1, color2);
  else if (type == "glow_ramp") actor.SetEffectGlowRamp(period, color1, color2);
  else if (type == "rainbow") actor.SetEffectRainbow(period);
  else if (type == "wag") actor.SetEffectWag(period, magnitude);
  else if (type == "bounce") actor.SetEffectBounce(period, magnitude);
  else if (type == "bob") actor.SetEffectBob(period, magnitude);
  else if (type == "pulse") {
    const auto zoom = effect.isMember("zoom")
        ? float_array(effect["zoom"], 2, path + ".zoom")
        : std::vector<float>{0.5f, 1.0f};
    actor.SetEffectPulse(period, zoom[0], zoom[1]);
    actor.SetEffectColor1(color1); actor.SetEffectColor2(color2);
  } else if (type == "spin") actor.SetEffectSpin(magnitude);
  else if (type == "vibrate") actor.SetEffectVibrate(magnitude);
  else throw std::runtime_error(path + ".type has unknown value `" + type + "`");
  actor.SetEffectClock(parse_clock(field_string(effect, "clock", "timer", path), path));
  actor.SetEffectOffset(field_number(effect, "offset", 0, path));
  if (effect.isMember("timing")) {
    const auto timing = float_array(effect["timing"], 5, path + ".timing");
    std::string error;
    if (!actor.SetEffectTiming(timing[0], timing[1], timing[2], timing[3],
                               timing[4], error)) {
      throw std::runtime_error(path + ".timing: " + error);
    }
  }
}

void apply_tweens(Actor& actor, const Json::Value& tweens,
                  const std::string& path) {
  if (tweens.isNull()) return;
  if (!tweens.isArray()) throw std::runtime_error(path + " must be an array");
  for (Json::ArrayIndex i = 0; i < tweens.size(); ++i) {
    const Json::Value& tween = tweens[i];
    const std::string item_path = path + "[" + std::to_string(i) + "]";
    if (!tween.isObject()) throw std::runtime_error(item_path + " must be an object");
    if (tween.isMember("queuemessage")) {
      const std::string message = field_string(tween, "queuemessage", "", item_path);
      if (message.empty()) throw std::runtime_error(item_path + ".queuemessage must be non-empty");
      actor.QueueMessage(message);
      continue;
    }
    if (tween.isMember("queuecommand")) {
      const std::string command = field_string(tween, "queuecommand", "", item_path);
      if (command.empty()) throw std::runtime_error(item_path + ".queuecommand must be non-empty");
      actor.QueueCommand(command);
      continue;
    }
    const float duration = field_number(tween, "duration", -1, item_path);
    if (duration < 0) throw std::runtime_error(item_path + ".duration must be non-negative");
    const std::string curve = field_string(tween, "curve", "linear", item_path);
    if (curve == "sleep") actor.Sleep(duration);
    else if (curve == "bezier") actor.BeginTweening(duration, make_bezier(tween, item_path));
    else actor.BeginTweening(duration, parse_curve(curve, item_path));
    if (tween.isMember("state")) apply_state(actor, tween["state"], item_path + ".state");
  }
}

struct ZoomSpline {
  Actor* actor;
  float beats_per_t;
  CubicSpline spline;
};

struct Scene {
  std::unique_ptr<HarnessFrame> root;
  std::vector<Actor*> actors;
  std::vector<ZoomSpline> zoom_splines;
  Json::Value sorts{Json::arrayValue};
  std::set<std::string> names;
};

Actor* build_actor(const Json::Value& spec, const std::string& path,
                   HarnessDisplay& display, Scene& scene) {
  if (!spec.isObject()) throw std::runtime_error(path + " must be an object");
  const std::string kind = field_string(spec, "kind", "sprite", path);
  Actor* actor;
  if (kind == "frame" || kind == "texture_frame") {
    ActorFrame* frame = kind == "texture_frame"
        ? static_cast<ActorFrame*>(new HarnessTextureFrame) : new HarnessFrame;
    frame->DeleteChildrenWhenDone(true);
    frame->SetDrawByZPosition(field_bool(spec, "draw_by_z_position", false, path));
    if (spec.isMember("camera")) {
      const Json::Value& camera = spec["camera"];
      if (!camera.isObject()) throw std::runtime_error(path + ".camera must be an object");
      frame->SetFOV(field_number(camera, "fov", 0, path + ".camera"));
      const auto vanish = camera.isMember("vanish")
          ? float_array(camera["vanish"], 2, path + ".camera.vanish")
          : std::vector<float>{g_screen_width / 2, g_screen_height / 2};
      frame->SetVanishPoint(vanish[0], vanish[1]);
    }
    actor = frame;
  } else if (kind == "sprite") {
    auto* sprite = new HarnessSprite;
    sprite->set_display(&display);
    sprite->Load(RageTextureID("__harness_actor_quad__.png"));
    if (spec.isMember("uv")) {
      const auto uv = float_array(spec["uv"], 4, path + ".uv");
      sprite->SetCustomTextureRect(RectF(uv[0], uv[1], uv[2], uv[3]));
    }
    actor = sprite;
  } else if (kind == "model") {
    auto model = std::make_unique<HarnessModel>();
    model->set_display(&display);
    const auto& pieces = spec["model_paths"];
    if (!pieces.isArray() || pieces.size() != 3)
      throw std::runtime_error(path + ".model_paths must contain meshes, materials and bones paths");
    for (const auto& piece : pieces) {
      if (!piece.isString()) throw std::runtime_error(path + ".model_paths entries must be strings");
    }
    model->LoadPieces(pieces[0].asString(), pieces[1].asString(), pieces[2].asString());
    actor = model.release();
  } else {
    throw std::runtime_error(path + ".kind must be `frame`, `texture_frame`, `sprite` or `model`");
  }
  const int index = static_cast<int>(scene.actors.size());
  if (auto* frame = dynamic_cast<HarnessFrame*>(actor)) {
    frame->bind_capture(&display, index);
    frame->bind_commands(spec["commands"], path + ".commands");
  }
  if (auto* frame = dynamic_cast<HarnessTextureFrame*>(actor)) {
    frame->bind_capture(&display, index);
    frame->bind_commands(spec["commands"], path + ".commands");
  }
  if (auto* sprite = dynamic_cast<HarnessSprite*>(actor)) {
    sprite->bind_capture(&display, index);
    sprite->bind_commands(spec["commands"], path + ".commands");
  }
  if (auto* model = dynamic_cast<HarnessModel*>(actor)) {
    model->bind_capture(&display, index);
    model->bind_commands(spec["commands"], path + ".commands");
  }
  if (!spec["subscriptions"].isNull()) {
    if (!spec["subscriptions"].isArray())
      throw std::runtime_error(path + ".subscriptions must be an array");
    for (const auto& value : spec["subscriptions"]) {
      if (!value.isString() || value.asString().empty())
        throw std::runtime_error(path + ".subscriptions entries must be non-empty strings");
      actor->SubscribeToMessage(value.asString());
    }
  }
  scene.actors.push_back(actor);
  apply_common(*actor, spec, path);
  if (auto* frame = dynamic_cast<HarnessTextureFrame*>(actor)) {
    frame->SetTextureName(actor->GetName());
    frame->Create();
  }
  if (spec.isMember("zoom_spline")) {
    const auto& zoom = spec["zoom_spline"];
    const auto& points = zoom["points"];
    if (!points.isArray() || points.empty() || points.size() > 65536)
      throw std::runtime_error(path + ".zoom_spline.points must contain 1..65536 values");
    ZoomSpline item{actor, field_number(zoom, "beats_per_t", 1, path), {}};
    if (item.beats_per_t <= 0) throw std::runtime_error("beats_per_t must be positive");
    item.spline.resize(points.size());
    for (Json::ArrayIndex i = 0; i < points.size(); ++i)
      item.spline.set_point(i, number(points[i], path + ".zoom_spline.points"));
    item.spline.solve_straight();
    scene.zoom_splines.push_back(std::move(item));
  }
  if (!actor->GetName().empty() && !scene.names.insert(actor->GetName()).second) {
    throw std::runtime_error("actor name `" + actor->GetName() + "` is duplicated");
  }
  apply_effect(*actor, spec["effect"], path + ".effect");
  apply_tweens(*actor, spec["tweens"], path + ".tweens");
  if (spec.isMember("wrappers")) {
    const auto& wrappers = spec["wrappers"];
    if (!wrappers.isArray()) throw std::runtime_error(path + ".wrappers must be an array");
    for (Json::ArrayIndex i = 0; i < wrappers.size(); ++i) {
      actor->AddWrapperState();
      Actor* wrapper = actor->GetWrapperState(i);
      const std::string wrapper_path = path + ".wrappers[" + std::to_string(i) + "]";
      apply_common(*wrapper, wrappers[i], wrapper_path);
      apply_effect(*wrapper, wrappers[i]["effect"], wrapper_path + ".effect");
      apply_tweens(*wrapper, wrappers[i]["tweens"], wrapper_path + ".tweens");
    }
  }
  if (auto* frame = dynamic_cast<ActorFrame*>(actor)) {
    const Json::Value& children = spec["children"];
    if (!children.isArray()) throw std::runtime_error(path + ".children must be an array");
    Json::Value sort(Json::objectValue);
    sort["frame"] = actor->GetName();
    sort["input"] = Json::Value(Json::arrayValue);
    for (Json::ArrayIndex i = 0; i < children.size(); ++i) {
      Actor* child = build_actor(children[i], path + ".children[" +
                                  std::to_string(i) + "]", display, scene);
      sort["input"].append(child->GetName());
      frame->AddChild(child);
    }
    const bool sort_enabled = field_bool(spec, "sort_by_draw_order", true, path);
    if (sort_enabled) frame->SortByDrawOrder();
    sort["enabled"] = sort_enabled;
    sort["post"] = Json::Value(Json::arrayValue);
    for (Actor* child : frame->GetChildren()) sort["post"].append(child->GetName());
    scene.sorts.append(std::move(sort));
  }
  return actor;
}

Json::Value actor_snapshot(Actor* actor) {
  if (auto* frame = dynamic_cast<HarnessFrame*>(actor)) return frame->snapshot("frame");
  if (auto* frame = dynamic_cast<HarnessTextureFrame*>(actor)) return frame->snapshot("texture_frame");
  if (auto* model = dynamic_cast<HarnessModel*>(actor)) return model->snapshot("model");
  return dynamic_cast<HarnessSprite*>(actor)->snapshot("sprite");
}

void bind_sample_actor(Actor* actor, Json::Value* records) {
  if (auto* frame = dynamic_cast<HarnessFrame*>(actor)) frame->bind_sample(records);
  else if (auto* frame = dynamic_cast<HarnessTextureFrame*>(actor)) frame->bind_sample(records);
  else if (auto* model = dynamic_cast<HarnessModel*>(actor)) model->bind_sample(records);
  else dynamic_cast<HarnessSprite*>(actor)->bind_sample(records);
}

// A model loaded by foreground Lua advances AnimatedTexture through Update;
// NoteDisplay instead seeks its cached model via SetSecondsIntoAnimation.
// Keep the material implementation native and feed Foreground::Update's
// start-relative, rate-unscaled deltas, including hidden intervals.
Json::Value evaluate_texture(const Json::Value& request) {
  const std::string texture_path = field_string(
      request, "animated_texture", "", "fixture");
  const Json::Value& cases = request["cases"];
  if (!cases.isArray() || cases.empty())
    throw std::runtime_error("texture fixture cases must be a non-empty array");
  Json::Value result(Json::objectValue);
  result["schema_version"] = 1;
  result["oracle"] = "itgmania_native_animated_texture";
  result["fixture"] = request["name"];
  result["animated_texture"] = texture_path;
  result["cases"] = Json::Value(Json::arrayValue);
  for (const auto& spec : cases) {
    AnimatedTexture texture;
    texture.Load(texture_path);
    const float start = field_number(spec, "start_second", 0, "case");
    const float rate = field_number(spec, "music_rate", 1, "case");
    if (rate <= 0) throw std::runtime_error("music_rate must be positive");
    Json::Value out = spec;
    out["samples"] = Json::Value(Json::arrayValue);
    double elapsed = 0;
    float last_music = start;
    for (const auto& value : spec["samples"]) {
      const float music = number(value, "sample music second");
      if (music < last_music && music >= start)
        throw std::runtime_error("texture samples must be ascending");
      // Foreground's first update starts at the authored FGCHANGE beat.
      const float age = std::max(music - start, 0.0f) / rate;
      while (elapsed < age) {
        const double step = std::min(1.0 / 120.0, double(age) - elapsed);
        texture.Update(static_cast<float>(step));
        elapsed += step;
      }
      Json::Value sample(Json::objectValue);
      sample["music_second"] = music;
      sample["animation_second"] = age;
      sample["texture_second"] = texture.GetSecondsIntoAnimation();
      sample["translation"] = vector2_json(texture.GetTextureTranslate());
      out["samples"].append(std::move(sample));
      last_music = music;
    }
    result["cases"].append(std::move(out));
  }
  return result;
}

Json::Value spline_samples(const Json::Value& fixtures) {
  Json::Value out(Json::arrayValue);
  for (const auto& fixture : fixtures) {
    const auto& points = fixture["points"];
    if (!points.isArray() || points.empty() || points.size() > 65536)
      throw std::runtime_error("spline points must contain 1..65536 vectors");
    CubicSpline axes[3];
    for (int axis = 0; axis < 3; ++axis) {
      axes[axis].resize(points.size());
      for (Json::ArrayIndex i = 0; i < points.size(); ++i)
        axes[axis].set_point(i, number(points[i][axis], "spline point"));
      axes[axis].solve_straight();
    }
    const float beats = field_number(fixture, "beats_per_t", 1, "spline");
    const float receptor = field_number(fixture, "receptor_t", 0, "spline");
    if (beats == 0) throw std::runtime_error("spline beats_per_t must be nonzero");
    const bool subtract = fixture.get("subtract_song_beat", true).asBool();
    Json::Value result = fixture;
    result["samples"] = Json::Value(Json::arrayValue);
    for (const auto& sample : fixture["sample_beats"]) {
      const float song = number(sample[0], "spline song beat");
      const float note = number(sample[1], "spline note beat");
      // NCSplineHandler::BeatToTValue and EvalForReceptor, NoteDisplay.cpp.
      const float t = subtract ? (note - song) / beats - receptor : note / beats;
      const float receptor_t = subtract ? receptor : song / beats;
      Json::Value row(Json::objectValue);
      row["song_beat"] = song;
      row["note_beat"] = note;
      for (int axis = 0; axis < 3; ++axis) {
        row["position"].append(axes[axis].evaluate(t, false));
        row["derivative"].append(axes[axis].evaluate_derivative(t, false));
        row["receptor"].append(axes[axis].evaluate(receptor_t, false));
      }
      result["samples"].append(std::move(row));
    }
    out.append(std::move(result));
  }
  return out;
}

Json::Value evaluate_lua_assertions(const Json::Value& request) {
  const char* key = request.isMember("lua_assertions") ? "lua_assertions" : "aft_creation";
  if (!request[key].isString())
    throw std::runtime_error(std::string(key) + " must be a Lua assertion body");
  HarnessDisplay display(640, 480);
  DISPLAY = &display;
  harness_clear_diagnostics();
  Json::Value result(Json::objectValue);
  {
    class ErrorListener final : public MessageSubscriber {
     public:
      Json::Value messages{Json::arrayValue};
      ~ErrorListener() override { UnsubscribeAll(); }
      void HandleMessage(const Message& message) override {
        std::string error;
        if (!message.GetParam("message", error))
          throw std::runtime_error("ScriptError message has no error parameter");
        messages.append(error);
      }
    } errors;
    errors.SubscribeToMessage("ScriptError");
    Lua* state = LUA->Get();
    const int stack = lua_gettop(state);
    luaL_openlibs(state);
    harness_register_lua_globals(state);
    ActorFrameTexture frozen, collision, unused;
    Actor target;
    ActorProxy proxy;
    BitmapText label;
    Sprite sprite;
    for (auto [name, actor] : {
        std::pair<const char*, Actor*>{"frozen", &frozen}, {"collision", &collision},
        {"unused", &unused}, {"native_target", &target}, {"native_proxy", &proxy},
        {"native_label", &label}, {"native_sprite", &sprite}}) {
      actor->PushSelf(state);
      lua_setglobal(state, name);
    }
    const auto script = request[key].asString();
    const bool failed = luaL_loadbuffer(state, script.data(), script.size(), key) != 0 ||
        lua_pcall(state, 0, 0, 0) != 0;
    const bool balanced = lua_gettop(state) == stack;
    const std::string error = failed ? lua_tostring(state, -1) : "";
    for (const char* name : {"frozen", "collision", "unused", "native_target", "native_proxy", "native_label", "native_sprite"}) {
      lua_pushnil(state);
      lua_setglobal(state, name);
    }
    lua_settop(state, stack);
    LUA->Release(state);
    if (failed) throw std::runtime_error(error);
    if (!balanced) throw std::runtime_error("actor assertions leaked Lua stack entries");
    result["script_errors"] = errors.messages;
    result["allocations"] = Json::Value(Json::arrayValue);
    for (const auto& param : display.allocations) {
      Json::Value allocation(Json::objectValue);
      allocation["width"] = param.iWidth;
      allocation["height"] = param.iHeight;
      allocation["alpha"] = param.bWithAlpha;
      allocation["depth"] = param.bWithDepthBuffer;
      allocation["float"] = param.bFloat;
      result["allocations"].append(std::move(allocation));
    }
    result["diagnostics"] = Json::Value(Json::arrayValue);
    for (const auto& diagnostic : harness_take_diagnostics()) result["diagnostics"].append(diagnostic);
  }
  DISPLAY = nullptr;
  return result;
}

Json::Value evaluate(const Json::Value& request) {
  if (!request.isObject()) throw std::runtime_error("fixture must be a JSON object");
  const int schema = request.get("schema_version", 1).asInt();
  if (schema != 1) throw std::runtime_error("unsupported actor fixture schema_version");
  if (request.isMember("aft_creation") || request.isMember("lua_assertions"))
    return evaluate_lua_assertions(request);
  if (request.isMember("animated_texture")) return evaluate_texture(request);
  const std::string name = field_string(request, "name", "unnamed", "fixture");
  const Json::Value& screen = request["screen"];
  g_screen_width = screen.isNull() ? 640 : field_number(screen, "width", 640, "screen");
  g_screen_height = screen.isNull() ? 480 : field_number(screen, "height", 480, "screen");
  if (g_screen_width <= 0 || g_screen_height <= 0) {
    throw std::runtime_error("screen dimensions must be positive");
  }
  const Json::Value& sample_values = request["samples"];
  if (!sample_values.isArray() || sample_values.empty()) {
    throw std::runtime_error("samples must be a non-empty array");
  }
  std::vector<float> samples;
  samples.reserve(sample_values.size());
  float prior = 0;
  for (Json::ArrayIndex i = 0; i < sample_values.size(); ++i) {
    const float sample = number(sample_values[i], "samples[" + std::to_string(i) + "]");
    if (sample < prior || sample < 0) {
      throw std::runtime_error("samples must be non-negative and ascending");
    }
    prior = sample;
    samples.push_back(sample);
  }
  HarnessDisplay display(static_cast<int>(g_screen_width),
                         static_cast<int>(g_screen_height));
  DISPLAY = &display;
  display.LoadMenuPerspective(0, g_screen_width, g_screen_height,
                              g_screen_width / 2, g_screen_height / 2);
  HarnessModelScope model_scope;
  Scene scene;
  Actor* root_actor = build_actor(request["root"], "root", display, scene);
  auto* root = dynamic_cast<HarnessFrame*>(root_actor);
  if (root == nullptr) throw std::runtime_error("root actor must be a frame");
  scene.root.reset(root);
  const uint32_t seed = request.get("random_seed", 1).asUInt();
  g_RandomNumberGenerator.seed(seed);
  const float bpm = field_number(request, "bpm", 120, "fixture");
  const float music_origin = field_number(request, "music_origin", 0, "fixture");
  Json::Value result(Json::objectValue);
  result["schema_version"] = 1;
  result["oracle"] = "itgmania_native_actor_conformance";
  result["fixture"] = name;
  if (request.isMember("splines")) result["splines"] = spline_samples(request["splines"]);
  result["random_seed"] = seed;
  result["screen"]["width"] = g_screen_width;
  result["screen"]["height"] = g_screen_height;
  result["post_draw_order"] = scene.sorts;
  result["samples"] = Json::Value(Json::arrayValue);
  float elapsed = 0;
  for (float sample_time : samples) {
    const float delta = sample_time - elapsed;
    elapsed = sample_time;
    const float beat = sample_time * bpm / 60.0f;
    // GameState supplies the music timestamp, while Actor::Update receives
    // elapsed wall time. The song offset must not change timer-clock deltas.
    const float music_seconds = sample_time + music_origin;
    Actor::SetBGMTime(music_seconds, beat, music_seconds, beat);
    Actor::SetPlayerBGMBeat(PLAYER_1, beat, beat);
    Actor::SetPlayerBGMBeat(PLAYER_2, beat, beat);
    root->Update(delta);
    // NoteColumnRenderer::UpdateReceptorGhostStuff applies offset zoom after
    // updating the actor. Both receptors and native ghosts use song beat here.
    for (const auto& zoom : scene.zoom_splines)
      zoom.actor->SetZoom(1.0f + zoom.spline.evaluate(beat / zoom.beats_per_t, false));
    Json::Value sample(Json::objectValue);
    sample["time"] = sample_time;
    sample["beat"] = beat;
    sample["actors"] = Json::Value(Json::arrayValue);
    for (Actor* actor : scene.actors) sample["actors"].append(actor_snapshot(actor));
    for (Actor* actor : scene.actors) bind_sample_actor(actor, &sample["actors"]);
    sample["draw_sequence"] = Json::Value(Json::arrayValue);
    display.start_sample(&sample["actors"], &sample["draw_sequence"]);
    root->Draw();
    result["samples"].append(std::move(sample));
  }
  // Render-target textures release their native display handles on destruction.
  scene.root.reset();
  DISPLAY = nullptr;
  return result;
}

ItgOracleBuffer json_buffer(const Json::Value& value) {
  Json::StreamWriterBuilder builder;
  builder["indentation"] = "";
  const std::string json = Json::writeString(builder, value);
  auto* data = new uint8_t[json.size()];
  std::memcpy(data, json.data(), json.size());
  return {data, json.size()};
}

}  // namespace

namespace ScreenDimensions {
float GetThemeAspectRatio() { return g_screen_width / g_screen_height; }
float GetScreenWidth() { return g_screen_width; }
float GetScreenHeight() { return g_screen_height; }
void ReloadScreenDimensions() {}
}  // namespace ScreenDimensions

namespace {
RageMatrix lua_matrix(lua_State* L, int index) {
  luaL_checktype(L, index, LUA_TTABLE);
  RageMatrix matrix;
  for (int row = 0; row < 4; ++row) {
    for (int column = 0; column < 4; ++column) {
      lua_rawgeti(L, index, row * 4 + column + 1);
      matrix.m[row][column] = static_cast<float>(luaL_checknumber(L, -1));
      lua_pop(L, 1);
    }
  }
  return matrix;
}

void push_matrix(lua_State* L, const RageMatrix& matrix) {
  lua_createtable(L, 16, 0);
  for (int row = 0; row < 4; ++row) {
    for (int column = 0; column < 4; ++column) {
      lua_pushnumber(L, matrix.m[row][column]);
      lua_rawseti(L, -2, row * 4 + column + 1);
    }
  }
}

RageVector4 lua_vector(lua_State* L, int index) {
  luaL_checktype(L, index, LUA_TTABLE);
  RageVector4 vector;
  for (int axis = 0; axis < 4; ++axis) {
    lua_rawgeti(L, index, axis + 1);
    vector[axis] = static_cast<float>(luaL_checknumber(L, -1));
    lua_pop(L, 1);
  }
  return vector;
}

// Actor::Update owns float hibernation subtraction and the wake-up remainder.
// Expose that phase to the semantic host without duplicating its clock math.
class HibernateStep final : public Actor {
 public:
  bool updated = false;
  float delta = 0;

 protected:
  void UpdateInternal(float elapsed) override {
    updated = true;
    delta = elapsed;
  }
};

class EffectMath final : public Actor {
 public:
  float spin_delta(float delta) {
    // A global music clock can produce a negative effect delta on its first
    // update. It is not a negative wall-time delta for Actor::Update.
    Actor::UpdateInternal(delta);
    return GetRotationZ();
  }

  TweenState sample_state(float units) {
    m_fSecsIntoEffect = units;
    PreDraw();
    const TweenState result = *m_pTempState;
    m_pTempState = nullptr;
    return result;
  }
};
}  // namespace

// One native model session owns geometry until the semantic Lua state closes.
// Every operation borrows the native singleton slots and restores them before
// returning to Lua; models never export manager-owned userdata to this state.
struct SongLuaModels {
  HarnessDisplay display{640, 480};
  std::unique_ptr<ModelManager> manager{std::make_unique<ModelManager>()};
  std::vector<std::unique_ptr<HarnessModel>> models;

  struct Globals {
    RageDisplay* display = DISPLAY;
    ModelManager* manager = MODELMAN;
    explicit Globals(SongLuaModels& session) {
      DISPLAY = &session.display;
      MODELMAN = session.manager.get();
    }
    ~Globals() { DISPLAY = display; MODELMAN = manager; }
  };

  SongLuaModels() {
    ModelManagerPrefs prefs;
    prefs.m_bDelayedUnload = false;
    manager->SetPrefs(prefs);
  }
  ~SongLuaModels() {
    Globals globals(*this);
    models.clear();
    manager.reset();
  }
  HarnessModel& get(int id) {
    if (id < 1 || static_cast<size_t>(id) > models.size())
      throw std::runtime_error("invalid native Model handle");
    return *models[id - 1];
  }
};

namespace {
void push_json(lua_State* state, const Json::Value& value) {
  if (value.isObject()) {
    lua_createtable(state, 0, value.size());
    if (value.empty()) {
      lua_createtable(state, 0, 1);
      lua_pushboolean(state, true); lua_setfield(state, -2, "_ITG_JSON_OBJECT");
      lua_setmetatable(state, -2);
    }
    for (const auto& key : value.getMemberNames()) {
      push_json(state, value[key]);
      lua_setfield(state, -2, key.c_str());
    }
  } else if (value.isArray()) {
    lua_createtable(state, value.size(), 0);
    int index = 0;
    for (const auto& item : value) {
      push_json(state, item);
      lua_rawseti(state, -2, ++index);
    }
  } else if (value.isString()) {
    const auto text = value.asString();
    lua_pushlstring(state, text.data(), text.size());
  } else if (value.isBool()) lua_pushboolean(state, value.asBool());
  else if (value.isNumeric()) lua_pushnumber(state, value.asDouble());
  else lua_pushnil(state);
}

SongLuaModels& model_session(lua_State* state) {
  return *static_cast<SongLuaModels*>(lua_touserdata(state, lua_upvalueindex(1)));
}

int model_load(lua_State* state) {
  const std::string mesh = luaL_optstring(state, 1, "");
  const std::string material = luaL_optstring(state, 2, "");
  const std::string bones = luaL_optstring(state, 3, "");
  try {
    auto& session = model_session(state);
    SongLuaModels::Globals globals(session);
    auto model = std::make_unique<HarnessModel>();
    model->set_display(&session.display);
    model->bind_capture(&session.display, 0);
    if (!mesh.empty() || !material.empty() || !bones.empty()) {
      if (mesh.empty() || material.empty() || bones.empty())
        throw std::runtime_error("Model requires all meshes, materials and bones pieces");
      model->LoadPieces(mesh, material, bones);
    }
    session.models.push_back(std::move(model));
    lua_pushinteger(state, session.models.size());
    return 1;
  } catch (const std::exception& error) {
    lua_pushnil(state); lua_pushstring(state, error.what()); return 2;
  }
}

int model_update(lua_State* state) {
  const int id = luaL_checkint(state, 1);
  const float delta = static_cast<float>(luaL_checknumber(state, 2));
  try {
    auto& session = model_session(state);
    SongLuaModels::Globals globals(session);
    session.get(id).Update(delta);
    lua_pushboolean(state, true); return 1;
  } catch (const std::exception& error) {
    lua_pushnil(state); lua_pushstring(state, error.what()); return 2;
  }
}

int model_call(lua_State* state) {
  const int id = luaL_checkint(state, 1);
  const char* name = luaL_checkstring(state, 2);
  const std::string_view method(name);
  Model* model;
  try { model = &model_session(state).get(id); }
  catch (const std::exception& error) {
    lua_pushnil(state); lua_pushstring(state, error.what()); return 2;
  }
  lua_remove(state, 1); lua_remove(state, 1);
  // These are the actual linked LunaModel argument checks and methods. Their
  // ignored self return is nil in this separate state (the macro above).
  try {
    if (method == "position") return LunaModel::position(model, state);
    if (method == "playanimation") return LunaModel::playanimation(model, state);
    if (method == "SetDefaultAnimation") return LunaModel::SetDefaultAnimation(model, state);
    if (method == "GetDefaultAnimation") return LunaModel::GetDefaultAnimation(model, state);
    if (method == "loop") return LunaModel::loop(model, state);
    if (method == "rate") return LunaModel::rate(model, state);
    if (method == "GetNumStates") return LunaModel::GetNumStates(model, state);
    // Match LunaActor's inherited argument contracts and virtual dispatch.
    // Pose and tween state remain in the semantic Actor, outside Model internals.
    lua_State* L = state;
    if (method == "animate") model->EnableAnimation(BIArg(1));
    else if (method == "play") model->EnableAnimation(true);
    else if (method == "pause") model->EnableAnimation(false);
    else if (method == "setstate") model->SetState(IArg(1));
    else if (method == "hibernate") model->SetHibernate(FArg(1));
    else if (method == "texturetranslate") model->SetTextureTranslate(FArg(1), FArg(2));
    else if (method == "texturewrapping") model->SetTextureWrapping(BIArg(1));
    else if (method == "SetTextureFiltering") model->SetTextureFiltering(BArg(1));
    else if (method == "blend") model->SetBlendMode(Enum::Check<BlendMode>(L, 1));
    else if (method == "zbuffer") model->SetUseZBuffer(BIArg(1));
    else if (method == "ztest") model->SetZTestMode(BIArg(1) ? ZTEST_WRITE_ON_PASS : ZTEST_OFF);
    else if (method == "ztestmode") model->SetZTestMode(Enum::Check<ZTestMode>(L, 1));
    else if (method == "zwrite") model->SetZWrite(BIArg(1));
    else if (method == "zbias") model->SetZBias(FArg(1));
    else if (method == "clearzbuffer") model->SetClearZBuffer(BIArg(1));
    else if (method == "backfacecull") model->SetCullMode(BIArg(1) ? CULL_BACK : CULL_NONE);
    else if (method == "cullmode") model->SetCullMode(Enum::Check<CullMode>(L, 1));
    else {
      lua_pushnil(state); lua_pushfstring(state, "unsupported native Model method: %s", name); return 2;
    }
    lua_pushboolean(state, true); return 1;
  } catch (const std::exception& error) {
    lua_pushnil(state); lua_pushstring(state, error.what()); return 2;
  }
}

int model_draw(lua_State* state) {
  const int id = luaL_checkint(state, 1);
  const auto diffuse = lua_vector(state, 2), glow = lua_vector(state, 3);
  try {
    auto& session = model_session(state);
    auto& model = session.get(id);
    model.SetDiffuse(RageColor(diffuse.x, diffuse.y, diffuse.z, diffuse.w));
    model.SetGlow(RageColor(glow.x, glow.y, glow.z, glow.w));
    Json::Value actors(Json::arrayValue), sequence(Json::arrayValue);
    actors.append(Json::Value(Json::objectValue));
    actors[0]["draws"] = Json::Value(Json::arrayValue);
    {
      SongLuaModels::Globals globals(session);
      struct Capture {
        HarnessDisplay& display;
        HarnessModel& model;
        ~Capture() { display.start_sample(nullptr, nullptr); model.bind_sample(nullptr); }
      } capture{session.display, model};
      model.bind_sample(&actors);
      session.display.start_sample(&actors, &sequence);
      model.Draw();
    }
    // Lua allocation failures must not strand borrowed native globals.
    push_json(state, actors[0]["draws"]);
    return 1;
  } catch (const std::exception& error) {
    lua_pushnil(state); lua_pushstring(state, error.what()); return 2;
  }
}
}  // namespace

SongLuaModels* install_song_models(lua_State* state) {
  auto session = std::make_unique<SongLuaModels>();
  for (const auto& entry : std::vector<std::pair<const char*, lua_CFunction>>{
      {"_ITG_MODEL_LOAD", model_load}, {"_ITG_MODEL_UPDATE", model_update},
      {"_ITG_MODEL_CALL", model_call}, {"_ITG_MODEL_DRAW", model_draw}}) {
    lua_pushlightuserdata(state, session.get());
    lua_pushcclosure(state, entry.second, 1);
    lua_setglobal(state, entry.first);
  }
  return session.release();
}

void destroy_song_models(SongLuaModels* models) { delete models; }

void install_actor_math(lua_State* state) {
  LuaDrawMode(state);
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    // Parse with the reference Lua bindings and capture DrawPrimitives, including
    // native topology limits and RageVColor quantization. The caller applies
    // its already captured parent/camera matrices to these local vertices.
    const RageVector4 diffuse = lua_vector(L, 4), glow = lua_vector(L, 5);
    const float line_width = static_cast<float>(luaL_checknumber(L, 3));
    HarnessDisplay display(640, 480);
    RageDisplay* previous_display = DISPLAY;
    Json::Value actors(Json::arrayValue), sequence(Json::arrayValue);
    actors.append(Json::Value(Json::objectValue));
    actors[0]["draws"] = Json::Value(Json::arrayValue);
    display.start_sample(&actors, &sequence);
    display.set_actor(0);
    {
      ActorMultiVertex actor;
      lua_pushvalue(L, 1);
      LunaActorMultiVertex::SetVertices(&actor, L);
      lua_settop(L, 5);
      lua_pushvalue(L, 2);
      lua_insert(L, 1);
      LunaActorMultiVertex::SetDrawState(&actor, L);
      lua_settop(L, 6);
      lua_remove(L, 1);
      actor.SetLineWidth(line_width);
      actor.SetDiffuse(RageColor(diffuse.x, diffuse.y, diffuse.z, diffuse.w));
      actor.SetGlow(RageColor(glow.x, glow.y, glow.z, glow.w));
      // Lua validation can longjmp. Install the stack-owned display only after
      // the reference setters have accepted the arguments.
      DISPLAY = &display;
      actor.Draw();
    }
    DISPLAY = previous_display;
    lua_createtable(L, actors[0]["draws"].size(), 0);
    int draw_index = 0;
    for (const auto& draw : actors[0]["draws"]) {
      lua_createtable(L, 0, 3);
      lua_pushstring(L, draw["primitive"].asCString());
      lua_setfield(L, -2, "primitive");
      lua_pushstring(L, draw["texture_mode"].asCString());
      lua_setfield(L, -2, "texture_mode");
      lua_createtable(L, draw["vertices"].size(), 0);
      int vertex_index = 0;
      for (const auto& vertex : draw["vertices"]) {
        lua_createtable(L, 0, 3);
        for (const char* key : {"local", "uv", "color"}) {
          lua_createtable(L, vertex[key].size(), 0);
          int channel = 0;
          for (const auto& value : vertex[key]) {
            lua_pushnumber(L, value.asDouble());
            lua_rawseti(L, -2, ++channel);
          }
          lua_setfield(L, -2, key);
        }
        lua_rawseti(L, -2, ++vertex_index);
      }
      lua_setfield(L, -2, "vertices");
      lua_rawseti(L, -2, ++draw_index);
    }
    return 1;
  }));
  lua_setglobal(state, "_ITG_AMV_DRAW");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    try {
      RageTexture* texture = TEXTUREMAN->LoadTexture(RageTextureID(luaL_checkstring(L, 1)));
      lua_createtable(L, 0, 9);
      const auto field = [L](const char* name, int value) {
        lua_pushinteger(L, value);
        lua_setfield(L, -2, name);
      };
      field("sourcewidth", texture->GetSourceWidth());
      field("sourceheight", texture->GetSourceHeight());
      field("sourceframewidth", texture->GetSourceFrameWidth());
      field("sourceframeheight", texture->GetSourceFrameHeight());
      field("texturewidth", texture->GetTextureWidth());
      field("textureheight", texture->GetTextureHeight());
      field("imagewidth", texture->GetImageWidth());
      field("imageheight", texture->GetImageHeight());
      field("numframes", texture->GetNumFrames());
      TEXTUREMAN->UnloadTexture(texture);
      return 1;
    } catch (const std::exception& error) {
      return luaL_error(L, "%s", error.what());
    }
  }));
  lua_setglobal(state, "_ITG_TEXTURE_INFO");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const RageTextureID id(luaL_checkstring(L, 1));
    lua_pushlstring(L, id.filename.data(), id.filename.size());
    return 1;
  }));
  lua_setglobal(state, "_ITG_TEXTURE_NAME");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const float percent = static_cast<float>(luaL_checknumber(L, 1));
    lua_pushinteger(L, TWEEN_BEZIER);
    lua_pushvalue(L, 2);
    std::unique_ptr<ITween> tween(ITween::CreateFromStack(L, 3));
    if (!tween) return luaL_error(L, "invalid Bezier controls");
    lua_pushnumber(L, tween->Tween(percent));
    return 1;
  }));
  lua_setglobal(state, "_ITG_BEZIER_PERCENT");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    // SongPosition's Lua bindings push native float fields as Lua numbers.
    lua_pushnumber(L, static_cast<float>(luaL_checknumber(L, 1)));
    return 1;
  }));
  lua_setglobal(state, "_ITG_FLOAT");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    HibernateStep actor;
    actor.SetHibernate(static_cast<float>(luaL_checknumber(L, 1)));
    actor.Update(static_cast<float>(luaL_checknumber(L, 2)));
    lua_pushnumber(L, actor.GetTweenTimeLeft());
    if (actor.updated) lua_pushnumber(L, actor.delta);
    else lua_pushnil(L);
    return 2;
  }));
  lua_setglobal(state, "_ITG_HIBERNATE_STEP");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const float from = static_cast<float>(luaL_checknumber(L, 1));
    const float to = static_cast<float>(luaL_checknumber(L, 2));
    const float percent = static_cast<float>(luaL_checknumber(L, 3));
    // Actor::TweenState::MakeWeightedAverage calls this native float lerp.
    lua_pushnumber(L, lerp(percent, from, to));
    return 1;
  }));
  lua_setglobal(state, "_ITG_ACTOR_LERP");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const float units = static_cast<float>(luaL_checknumber(L, 1));
    const auto values = [L](int index, int count) {
      luaL_checktype(L, index, LUA_TTABLE);
      std::vector<float> result;
      result.reserve(count);
      for (int axis = 0; axis < count; ++axis) {
        lua_rawgeti(L, index, axis + 1);
        result.push_back(static_cast<float>(luaL_checknumber(L, -1)));
        lua_pop(L, 1);
      }
      return result;
    };
    const auto zoom = values(2, 3), magnitude = values(3, 3);
    const RageVector4 color1 = lua_vector(L, 4), color2 = lua_vector(L, 5);
    const auto timing = values(6, 5);
    const float offset = static_cast<float>(luaL_checknumber(L, 7));
    EffectMath actor;
    actor.SetZoomX(zoom[0]); actor.SetZoomY(zoom[1]); actor.SetZoomZ(zoom[2]);
    actor.SetEffectPulse(1, magnitude[0], magnitude[1]);
    actor.SetEffectColor1(RageColor(color1.x, color1.y, color1.z, color1.w));
    actor.SetEffectColor2(RageColor(color2.x, color2.y, color2.z, color2.w));
    actor.SetEffectOffset(offset);
    std::string error;
    if (!actor.SetEffectTiming(timing[0], timing[1], timing[2], timing[3], timing[4], error))
      return luaL_error(L, "%s", error.c_str());
    const RageVector3 scale = actor.sample_state(units).scale;
    lua_createtable(L, 3, 0);
    for (int axis = 0; axis < 3; ++axis) {
      lua_pushnumber(L, scale[axis]);
      lua_rawseti(L, -2, axis + 1);
    }
    return 1;
  }));
  lua_setglobal(state, "_ITG_PULSE_ZOOM");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const std::string mode = luaL_checkstring(L, 1);
    const float units = static_cast<float>(luaL_checknumber(L, 2));
    const auto vector3 = [L](int index) {
      luaL_checktype(L, index, LUA_TTABLE);
      RageVector3 vector;
      for (int axis = 0; axis < 3; ++axis) {
        lua_rawgeti(L, index, axis + 1);
        vector[axis] = static_cast<float>(luaL_checknumber(L, -1));
        lua_pop(L, 1);
      }
      return vector;
    };
    const RageVector3 position = vector3(3), rotation = vector3(4);
    const RageVector3 vector = vector3(5);
    EffectMath actor;
    actor.SetX(position.x); actor.SetY(position.y); actor.SetZ(position.z);
    actor.SetRotationX(rotation.x); actor.SetRotationY(rotation.y);
    actor.SetRotationZ(rotation.z);
    if (mode == "bob") actor.SetEffectBob(1, vector);
    else if (mode == "bounce") actor.SetEffectBounce(1, vector);
    else if (mode == "wag") actor.SetEffectWag(1, vector);
    else return luaL_error(L, "unknown motion effect: %s", mode.c_str());
    luaL_checktype(L, 6, LUA_TTABLE);
    std::array<float, 5> timing;
    for (int index = 0; index < 5; ++index) {
      lua_rawgeti(L, 6, index + 1);
      timing[index] = static_cast<float>(luaL_checknumber(L, -1));
      lua_pop(L, 1);
    }
    std::string error;
    if (!actor.SetEffectTiming(timing[0], timing[1], timing[2], timing[3], timing[4], error))
      return luaL_error(L, "%s", error.c_str());
    actor.SetEffectOffset(static_cast<float>(luaL_checknumber(L, 7)));
    const auto sample = actor.sample_state(units);
    const std::array<RageVector3, 2> pose = {sample.pos, sample.rotation};
    for (int component = 0; component < 2; ++component) {
      lua_createtable(L, 3, 0);
      for (int axis = 0; axis < 3; ++axis) {
        lua_pushnumber(L, pose[component][axis]);
        lua_rawseti(L, -2, axis + 1);
      }
    }
    return 2;
  }));
  lua_setglobal(state, "_ITG_MOTION_POSE");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const std::string mode = luaL_checkstring(L, 1);
    const float units = static_cast<float>(luaL_checknumber(L, 2));
    const auto color = [L](int index) {
      const RageVector4 v = lua_vector(L, index);
      return RageColor(v.x, v.y, v.z, v.w);
    };
    EffectMath actor;
    actor.SetDiffuse(color(3)); actor.SetGlow(color(4));
    const RageColor color1 = color(5), color2 = color(6);
    if (mode == "diffuseblink") actor.SetEffectDiffuseBlink(1, color1, color2);
    else if (mode == "diffuseshift") actor.SetEffectDiffuseShift(1, color1, color2);
    else if (mode == "diffuseramp") actor.SetEffectDiffuseRamp(1, color1, color2);
    else if (mode == "glowblink") actor.SetEffectGlowBlink(1, color1, color2);
    else if (mode == "glowshift") actor.SetEffectGlowShift(1, color1, color2);
    else if (mode == "glowramp") actor.SetEffectGlowRamp(1, color1, color2);
    else if (mode == "rainbow") actor.SetEffectRainbow(1);
    else if (mode != "none") return luaL_error(L, "unknown color effect: %s", mode.c_str());
    luaL_checktype(L, 7, LUA_TTABLE);
    std::array<float, 5> timing;
    for (int index = 0; index < 5; ++index) {
      lua_rawgeti(L, 7, index + 1);
      timing[index] = static_cast<float>(luaL_checknumber(L, -1));
      lua_pop(L, 1);
    }
    std::string error;
    if (!actor.SetEffectTiming(timing[0], timing[1], timing[2], timing[3], timing[4], error))
      return luaL_error(L, "%s", error.c_str());
    actor.SetEffectOffset(static_cast<float>(luaL_checknumber(L, 8)));
    actor.SetInternalDiffuse(color(9)); actor.SetInternalGlow(color(10));
    const auto sample = actor.sample_state(units);
    for (const RageColor& output : {sample.diffuse[0], sample.glow}) {
      lua_createtable(L, 4, 0);
      for (int channel = 0; channel < 4; ++channel) {
        lua_pushnumber(L, output[channel]);
        lua_rawseti(L, -2, channel + 1);
      }
    }
    return 2;
  }));
  lua_setglobal(state, "_ITG_ACTOR_COLORS");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    EffectMath actor;
    actor.SetRotationZ(static_cast<float>(luaL_checknumber(L, 1)));
    actor.SetEffectSpin(RageVector3(0, 0, static_cast<float>(luaL_checknumber(L, 2))));
    lua_pushnumber(L, actor.spin_delta(static_cast<float>(luaL_checknumber(L, 3))));
    return 1;
  }));
  lua_setglobal(state, "_ITG_SPIN_ROTATION");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const RageMatrix left = lua_matrix(L, 1), right = lua_matrix(L, 2);
    RageMatrix result;
    // RageMatrixMultiply's arguments are reversed for row-vector storage.
    RageMatrixMultiply(&result, &right, &left);
    push_matrix(L, result);
    return 1;
  }));
  lua_setglobal(state, "_ITG_MATRIX_MUL");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    RageMatrix result;
    RageMatrixRotationXYZ(&result, static_cast<float>(luaL_checknumber(L, 1)),
        static_cast<float>(luaL_checknumber(L, 2)), static_cast<float>(luaL_checknumber(L, 3)));
    push_matrix(L, result);
    return 1;
  }));
  lua_setglobal(state, "_ITG_ROTATION_MATRIX");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const RageVector4 vector = lua_vector(L, 1);
    const RageMatrix matrix = lua_matrix(L, 2);
    RageVector4 result;
    RageVec4TransformCoord(&result, &vector, &matrix);
    lua_createtable(L, 4, 0);
    for (int axis = 0; axis < 4; ++axis) {
      lua_pushnumber(L, result[axis]);
      lua_rawseti(L, -2, axis + 1);
    }
    return 1;
  }));
  lua_setglobal(state, "_ITG_VECTOR_TRANSFORM");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const float width = static_cast<float>(luaL_checknumber(L, 1));
    const float height = static_cast<float>(luaL_checknumber(L, 2));
    const float fov = static_cast<float>(luaL_checknumber(L, 3));
    const float x = static_cast<float>(luaL_checknumber(L, 4));
    const float y = static_cast<float>(luaL_checknumber(L, 5));
    HarnessDisplay display(static_cast<int>(width), static_cast<int>(height));
    display.LoadMenuPerspective(fov, width, height, x, y);
    push_matrix(L, display.view());
    push_matrix(L, display.projection());
    lua_pushnumber(L, -display.view().m[3][2]);
    return 3;
  }));
  lua_setglobal(state, "_ITG_MENU_MATRICES");
  lua_pushcfunction(state, ([](lua_State* L) -> int {
    const RageVector4 clip = lua_vector(L, 1);
    const float width = static_cast<float>(luaL_checknumber(L, 2));
    const float height = static_cast<float>(luaL_checknumber(L, 3));
    const float inverse_w = clip.w == 0 ? 0 : 1.0f / clip.w;
    lua_createtable(L, 2, 0);
    lua_pushnumber(L, (clip.x * inverse_w + 1) * width * 0.5f);
    lua_rawseti(L, -2, 1);
    lua_pushnumber(L, (1 - clip.y * inverse_w) * height * 0.5f);
    lua_rawseti(L, -2, 2);
    return 1;
  }));
  lua_setglobal(state, "_ITG_SCREEN_VERTEX");
}

extern "C" ItgOracleBuffer itg_oracle_eval_actor_fixture(
    const uint8_t* request, size_t request_len) {
  try {
    const std::lock_guard<std::mutex> guard(harness_native_mutex());
    Json::CharReaderBuilder builder;
    Json::Value parsed;
    std::string errors;
    const std::unique_ptr<Json::CharReader> reader(builder.newCharReader());
    const char* begin = reinterpret_cast<const char*>(request);
    if (!reader->parse(begin, begin + request_len, &parsed, &errors)) {
      throw std::runtime_error("invalid actor fixture JSON: " + errors);
    }
    return json_buffer(evaluate(parsed));
  } catch (const std::exception& error) {
    DISPLAY = nullptr;
    Json::Value result(Json::objectValue);
    result["error"] = error.what();
    return json_buffer(result);
  }
}
