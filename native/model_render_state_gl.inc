// Included after RageDisplay_OGL.cpp in the generated observational source.
// Read the context at the actual compiled-mesh draw, after Model overrides and
// native matrix setup. Restore the active texture unit before submitting it.
void itgmania_harness_draw_model_gl_current(int index,
    const RageMatrix& world, const RageMatrix& view, const RageMatrix& projection) {
  if (!itgmania_harness_model_capture_enabled()) return;
  ItgmaniaHarnessModelGlState state{};
  state.lighting = glIsEnabled(GL_LIGHTING) != 0;
  state.blend = glIsEnabled(GL_BLEND) != 0;
  state.depth_test = glIsEnabled(GL_DEPTH_TEST) != 0;
  state.cull = glIsEnabled(GL_CULL_FACE) != 0;
  state.alpha_test = glIsEnabled(GL_ALPHA_TEST) != 0;
  GLboolean write;
  glGetBooleanv(GL_DEPTH_WRITEMASK, &write); state.depth_write = write != 0;
  glGetIntegerv(GL_DEPTH_FUNC, &state.depth_function);
  glGetFloatv(GL_DEPTH_RANGE, state.depth_range);
  glGetIntegerv(GL_CULL_FACE_MODE, &state.cull_face);
  glGetIntegerv(GL_ALPHA_TEST_FUNC, &state.alpha_function);
  glGetFloatv(GL_ALPHA_TEST_REF, &state.alpha_reference);
  glGetFloatv(GL_CURRENT_COLOR, state.current_color);
  if (GLEW_EXT_blend_func_separate) {
    glGetIntegerv(GL_BLEND_SRC_RGB_EXT, &state.blend_src_rgb);
    glGetIntegerv(GL_BLEND_DST_RGB_EXT, &state.blend_dst_rgb);
    glGetIntegerv(GL_BLEND_SRC_ALPHA_EXT, &state.blend_src_alpha);
    glGetIntegerv(GL_BLEND_DST_ALPHA_EXT, &state.blend_dst_alpha);
  } else {
    glGetIntegerv(GL_BLEND_SRC, &state.blend_src_rgb);
    glGetIntegerv(GL_BLEND_DST, &state.blend_dst_rgb);
    state.blend_src_alpha = state.blend_src_rgb;
    state.blend_dst_alpha = state.blend_dst_rgb;
  }
  if (glBlendEquation != nullptr) glGetIntegerv(GL_BLEND_EQUATION, &state.blend_equation_rgb);
  else state.blend_equation_rgb = GL_FUNC_ADD;
  if (GLEW_EXT_blend_equation_separate)
    glGetIntegerv(GL_BLEND_EQUATION_ALPHA_EXT, &state.blend_equation_alpha);
  else state.blend_equation_alpha = state.blend_equation_rgb;
  GLint active = GL_TEXTURE0_ARB, units = 1;
  if (GLEW_ARB_multitexture) {
    glGetIntegerv(GL_ACTIVE_TEXTURE_ARB, &active);
    glGetIntegerv(GL_MAX_TEXTURE_UNITS_ARB, &units);
  }
  state.texture_units = std::min(4, units);
  for (int unit = 0; unit < state.texture_units; ++unit) {
    if (GLEW_ARB_multitexture) glActiveTextureARB(GL_TEXTURE0_ARB + unit);
    auto& texture = state.textures[unit];
    texture.enabled = glIsEnabled(GL_TEXTURE_2D) != 0;
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &texture.handle);
    glGetTexEnviv(GL_TEXTURE_ENV, GL_TEXTURE_ENV_MODE, &texture.environment_mode);
  }
  if (GLEW_ARB_multitexture) glActiveTextureARB(active);
  itgmania_harness_draw_model_gl(state, index, world, view, projection);
}
