-- Native note geometry only. Do not wrap authored functions or derive poses
-- from Lua writes: observe submitted meshes and the renderer's actual clocks.
local cfg=ITGMANIA_HARNESS_CONFIG
if cfg.stop_lua_gc then collectgarbage('stop') end
assert(ITGMANIA_HARNESS_MODEL_GEOMETRY_PROTOCOL_V2==2,"native note geometry adapter v2 required")
ITGMANIA_HARNESS_NOTEFIELD_ONLY=true
ITGMANIA_HARNESS_CAPTURE_FROM_BEAT=cfg.drawable_from_beat or 0
local models,meshes,strips,model_frames,strip_frames,frames={},{},{},{},{},{}
local actor_ids,actors=setmetatable({},{__mode='k'}),{}
local scene,frame,sampled_frame=0,0,-1
local delta,pending,finished=0,nil,false
local inputs={}
local dropped_models,dropped_strips=0,0
local function actor_id(actor)
 local id=actor_ids[actor]
 if not id then id='native-'..tostring(#actors+1);actor_ids[actor]=id;actors[#actors+1]={id=id,name=actor:GetName()} end
 return id
end
local function position()
 local p=GAMESTATE:GetSongPosition()
 return p:GetSongBeatVisible(),p:GetMusicSeconds()
end
local function capture_now()
 return not finished and scene>0 and GAMESTATE:GetSongPosition():GetSongBeatVisible()>=(cfg.drawable_from_beat or 0)
end
local function anchor()
 return sampled_frame==frame and #frames or #frames+1
end
local function sample_update()
 if finished or sampled_frame==frame then return end
 sampled_frame=frame
 local beat,seconds=position()
 frames[#frames+1]={sequence=#frames+1,scene=scene,native_update_frame=frame,
  delta=delta,beat=beat,seconds=seconds,wall_seconds=GetTimeSinceStart()}
end
ITGMANIA_HARNESS_MODEL_MESH=function(id,vertices)
 meshes[#meshes+1]={id=id,primitive='triangles',vertices=vertices}
end
ITGMANIA_HARNESS_DRAW_MODEL=function(actor,geometry,state)
 if not capture_now() then return end
 if #models>=cfg.max_events then dropped_models=dropped_models+1;return end
 local beat=position()
 models[#models+1]={scene=scene,update_sequence=anchor(),beat=beat,
  actor=actor_id(actor),geometry=geometry,primitive='ModelTriangles',draw_phase=state.draw_phase,
  coordinate_space='native-projection-before-display-centering',state=state}
end
ITGMANIA_HARNESS_DRAW_VERTICES=function(actor,primitive,vertices,state)
 if not capture_now() or not state.note_context then return end
 if #strips>=cfg.max_events then dropped_strips=dropped_strips+1;return end
 strips[#strips+1]={scene=scene,update_sequence=anchor(),actor=actor_id(actor),primitive=primitive,
  coordinate_space='native-projection-before-display-centering',vertices=vertices,state=state}
end
ITGMANIA_HARNESS_BEFORE_SCREEN_UPDATE=function(dt)
 frame=frame+1;delta=dt
 if scene>0 and cfg.capture_frame_period_seconds then ITGMANIA_HARNESS_PACE_CAPTURE(cfg.capture_frame_period_seconds) end
end
ITGMANIA_HARNESS_AFTER_SCREEN_DRAW=function()
 if finished or scene==0 then return end
 sample_update()
 if capture_now() then
  model_frames[#model_frames+1]={scene=scene,update_sequence=#frames,samples=#models}
  strip_frames[#strip_frames+1]={scene=scene,update_sequence=#frames,samples=#strips}
 end
 if not pending then return end
 finished=true
 local beat,seconds=position()
 local file=RageFileUtil:CreateRageFile()
 assert(file:Open(cfg.result_path,2),'cannot open note geometry trace')
 file:Write(JsonEncode({schema_version=1,oracle='itgmania_notefield_render_trace',
  harness_version=cfg.harness_version,itgmania_version=ProductVersion(),
  title=GAMESTATE:GetCurrentSong():GetDisplayFullTitle(),style=cfg.style,difficulty=cfg.difficulty,
  display={logical_width=SCREEN_WIDTH,logical_height=SCREEN_HEIGHT,width=DISPLAY:GetDisplayWidth(),height=DISPLAY:GetDisplayHeight(),video_renderers=PREFSMAN:GetPreference('VideoRenderers')},
  input={notefield_samples=true,player_options=cfg.player_options,players=inputs,autoplay=cfg.autoplay,
   lua_gc_mode=cfg.stop_lua_gc and 'stopped' or 'automatic',
   drawable_from_beat=cfg.drawable_from_beat,observer_frame_period_seconds=cfg.capture_frame_period_seconds,
   headless_graphics=ITGMANIA_HARNESS_HEADLESS_GRAPHICS==true,
   window_visible=ITGMANIA_HARNESS_WINDOW_VISIBLE(),
   constant_update_delta_seconds=PREFSMAN:GetPreference('ConstantUpdateDeltaSeconds')},
  capabilities={native_notefield_geometry=true,native_drawable_vertices=true,native_model_geometry_protocol=2},
  capture_limits={max_events=cfg.max_events},runtime_actors=actors,update_frames=frames,
  model_geometry_meshes=meshes,model_geometry_samples=models,model_geometry_frames=model_frames,
  drawable_samples=strips,drawable_frames=strip_frames,
  dropped_model_geometry_samples=dropped_models,dropped_drawable_samples=dropped_strips,
  interactive_observer_error=ITGMANIA_HARNESS_INTERACTIVE_ERROR,
  completion=pending,end_position={beat=beat,seconds=seconds},random_seed=cfg.random_seed}))
 file:Close();file:destroy()
end
local function noop() end
ITGMANIA_HARNESS_TRACE={capture_catalog_inputs=noop,
 capture_inputs=function()
  for _,player in ipairs(GAMESTATE:GetEnabledPlayers()) do
   local opts=GAMESTATE:GetPlayerState(player):GetPlayerOptions('ModsLevel_Current')
   inputs[#inputs+1]={player=tostring(player),options_string=GAMESTATE:GetPlayerState(player):GetPlayerOptionsString('ModsLevel_Current'),mini=opts:Mini(),noteskin=opts:NoteSkin()}
  end
 end,
 seed_random=function() MersenneTwister.Seed(cfg.random_seed) end,
 scene_init=function() scene=scene+1;sampled_frame=-1 end,
 sample_update=sample_update,sample_stage=noop,screen_changed=noop,
 finish=function(reason) pending=reason end}
