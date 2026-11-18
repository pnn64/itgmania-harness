-- Native note geometry only. Do not wrap authored functions or derive poses
-- from Lua writes: observe submitted meshes and the renderer's actual clocks.
local cfg=ITGMANIA_HARNESS_CONFIG
if cfg.stop_lua_gc then collectgarbage('stop') end
assert(ITGMANIA_HARNESS_MODEL_GEOMETRY_PROTOCOL_V2==2,"native note geometry adapter v2 required")
assert(ITGMANIA_HARNESS_MODEL_RENDER_STATE_PROTOCOL_V1==1,"native Model render-state adapter v1 required")
assert(ITGMANIA_HARNESS_RECEPTOR_GEOMETRY_PROTOCOL_V1==1,"native receptor geometry adapter v1 required")
assert(type(ITGMANIA_HARNESS_OPTION_DELTAS)=='function',"native option clock adapter required")
ITGMANIA_HARNESS_NOTEFIELD_ONLY=not cfg.scene_draws
ITGMANIA_HARNESS_CAPTURE_FROM_BEAT=cfg.drawable_from_beat or 0
local models,meshes,strips,model_frames,strip_frames,frames={},{},{},{},{},{}
local receptor_visits,receptor_frames={},{}
local player_positions={}
local actor_ids,actors=setmetatable({},{__mode='k'}),{}
local scene,frame,sampled_frame=0,0,-1
local arrow_frame,arrow_inputs=-1,nil
local delta,pending,finished=0,nil,false
local inputs={}
local input_preferences,catalog_preferences=nil,nil
local fast_note_rendering=nil
local dropped_models,dropped_strips=0,0
local dropped_receptors=0
local current_option_methods={perspective_tilt='Tilt',skew='Skew',mini='Mini',brake='Brake',drunk='Drunk',tipsy='Tipsy'}
local function current_options(player)
 local options=GAMESTATE:GetPlayerState(player):GetPlayerOptions('ModsLevel_Current')
 local result={approach_speeds={},perspective_getters={}}
 for key,method in pairs(current_option_methods) do
  -- No arguments: FLOAT_INTERFACE returns its current value and approach speed.
  local value,speed=options[method](options)
  result[key]=value;result.approach_speeds[key]=speed
 end
 for _,method in ipairs({'Hallway','Distant','Incoming','Space'}) do
  -- These aliases may return nil,nil even for a nonzero canonical Tilt/Skew.
  local value,speed=options[method](options)
  result.perspective_getters[method]={present=value~=nil,value=value,approach_speed=speed}
 end
 return result
end
local function actor_id(actor)
 local id=actor_ids[actor]
 if not id then id='native-'..tostring(#actors+1);actor_ids[actor]=id;actors[#actors+1]={id=id,name=actor:GetName()} end
 return id
end
local function position()
 local p=GAMESTATE:GetSongPosition()
 return p:GetSongBeatVisible(),p:GetMusicSeconds(),p:GetSongBeat(),p:GetMusicSecondsVisible()
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
 local beat,seconds,song_beat,visible_seconds=position()
 -- GameState advances SongOptions and both PlayerOptions before Screen::Update.
 -- These are the actual ModsGroup timer inputs used by this frame's draws.
 frames[#frames+1]={sequence=#frames+1,scene=scene,native_update_frame=frame,
  delta=delta,beat=beat,seconds=seconds,song_beat=song_beat,
  visible_beat=beat,visible_seconds=visible_seconds,wall_seconds=GetTimeSinceStart(),
  option_deltas=ITGMANIA_HARNESS_OPTION_DELTAS(),
  arrow_clock_inputs=arrow_frame==frame and arrow_inputs or nil}
end
-- The overlay normally samples before ScreenGameplay's final ArrowEffects
-- update; after-draw sampling is also supported. Never attach a prior frame's
-- cache inputs or replace the observed clocks with an observer timer read.
ITGMANIA_HARNESS_AFTER_ARROW_UPDATE=function(inputs)
 if finished or scene==0 then return end
 arrow_frame,arrow_inputs=frame,inputs
 if sampled_frame==frame then frames[#frames].arrow_clock_inputs=inputs end
end
ITGMANIA_HARNESS_MODEL_MESH=function(id,vertices)
 meshes[#meshes+1]={id=id,primitive='triangles',vertices=vertices}
end
ITGMANIA_HARNESS_DRAW_MODEL=function(actor,geometry,state)
 if not capture_now() then return end
 if #models>=cfg.max_events then dropped_models=dropped_models+1;return end
 local beat=GAMESTATE:GetSongPosition():GetSongBeatVisible()
 models[#models+1]={scene=scene,update_sequence=anchor(),beat=beat,
  actor=actor_id(actor),geometry=geometry,primitive='ModelTriangles',draw_phase=state.draw_phase,
  coordinate_space='native-projection-before-display-centering',state=state}
end
ITGMANIA_HARNESS_DRAW_VERTICES=function(actor,primitive,vertices,state)
 if not capture_now() or not cfg.scene_draws and not state.note_context then return end
 if #strips>=cfg.max_events then dropped_strips=dropped_strips+1;return end
 strips[#strips+1]={scene=scene,update_sequence=anchor(),actor=actor_id(actor),primitive=primitive,
  coordinate_space='native-projection-before-display-centering',vertices=vertices,state=state}
end
ITGMANIA_HARNESS_RECEPTOR_VISIT=function(actor,state)
 if not capture_now() then return end
 if #receptor_visits>=cfg.max_events then dropped_receptors=dropped_receptors+1;return end
 receptor_visits[#receptor_visits+1]={scene=scene,update_sequence=anchor(),actor=actor_id(actor),state=state}
end
ITGMANIA_HARNESS_BEFORE_SCREEN_UPDATE=function(dt)
 frame=frame+1;delta=dt
 arrow_frame,arrow_inputs=-1,nil
 if scene>0 and cfg.capture_frame_period_seconds then ITGMANIA_HARNESS_PACE_CAPTURE(cfg.capture_frame_period_seconds) end
end
ITGMANIA_HARNESS_AFTER_SCREEN_DRAW=function()
 if finished or scene==0 then return end
 sample_update()
 if capture_now() then
  local screen=SCREENMAN:GetTopScreen()
  local positions={}
  for _,player in ipairs(GAMESTATE:GetEnabledPlayers()) do
   local index=player==PLAYER_1 and 0 or 1
   local name='PlayerP'..tostring(index+1)
   local actor=assert(screen:GetChild(name),'native Player actor missing')
   -- GetX/Y/Z read Actor::m_current.pos. GetZoom reads DestTweenState and is
   -- deliberately excluded from this current-position observation.
   positions[#positions+1]={player=index,name=name,x=actor:GetX(),y=actor:GetY(),z=actor:GetZ(),
    current_options=current_options(player)}
  end
  player_positions[#player_positions+1]={scene=scene,update_sequence=#frames,
   native_update_frame=frame,players=positions}
  model_frames[#model_frames+1]={scene=scene,update_sequence=#frames,samples=#models}
  strip_frames[#strip_frames+1]={scene=scene,update_sequence=#frames,samples=#strips}
  receptor_frames[#receptor_frames+1]={scene=scene,update_sequence=#frames,samples=#receptor_visits}
 end
 if not pending then return end
 finished=true
 local beat,seconds,song_beat,visible_seconds=position()
 local file=RageFileUtil:CreateRageFile()
 local script_errors,dropped_script_errors=ITGMANIA_HARNESS_SCRIPT_ERRORS()
 assert(file:Open(cfg.result_path,2),'cannot open note geometry trace')
 file:Write(JsonEncode({schema_version=1,oracle='itgmania_notefield_render_trace',
  harness_version=cfg.harness_version,itgmania_version=ProductVersion(),
  title=GAMESTATE:GetCurrentSong():GetDisplayFullTitle(),style=cfg.style,difficulty=cfg.difficulty,
  display={logical_width=SCREEN_WIDTH,logical_height=SCREEN_HEIGHT,width=DISPLAY:GetDisplayWidth(),height=DISPLAY:GetDisplayHeight(),video_renderers=PREFSMAN:GetPreference('VideoRenderers')},
  input={notefield_samples=true,scene_draws=cfg.scene_draws==true,player_options=cfg.player_options,players=inputs,autoplay=cfg.autoplay,
   preferences=input_preferences,catalog_preferences=catalog_preferences,
   fast_note_rendering_requested=cfg.fast_note_rendering,fast_note_rendering=fast_note_rendering,
   fast_note_rendering_at_end=PREFSMAN:GetPreference('FastNoteRendering'),
   native_source=ITGMANIA_HARNESS_NATIVE_SOURCE,
   lua_gc_mode=cfg.stop_lua_gc and 'stopped' or 'automatic',
   drawable_from_beat=cfg.drawable_from_beat,observer_frame_period_seconds=cfg.capture_frame_period_seconds,
   headless_graphics=ITGMANIA_HARNESS_HEADLESS_GRAPHICS==true,
   window_visible=ITGMANIA_HARNESS_WINDOW_VISIBLE(),
   constant_update_delta_seconds=PREFSMAN:GetPreference('ConstantUpdateDeltaSeconds')},
  capabilities={native_notefield_geometry=true,native_drawable_vertices=true,native_model_geometry_protocol=2,native_receptor_geometry_protocol=1,
   native_model_render_state_protocol=ITGMANIA_HARNESS_MODEL_RENDER_STATE_BACKEND()=='opengl' and 1 or nil,
   native_model_render_state_backend=ITGMANIA_HARNESS_MODEL_RENDER_STATE_BACKEND(),
   native_song_position_clocks=true,native_pre_theme_preferences=catalog_preferences~=nil,
   native_option_clock_deltas=true,
   native_arrow_update_clocks=type(ITGMANIA_HARNESS_ARROW_CLOCK_SCOPE)=='function',
   native_player_actor_positions=true,
   native_player_current_options=true,
   native_fast_note_rendering=type(fast_note_rendering)=='boolean'},
  capture_limits={max_events=cfg.max_events},runtime_actors=actors,update_frames=frames,
  player_actor_position_samples=player_positions,
  model_geometry_meshes=meshes,model_geometry_samples=models,model_geometry_frames=model_frames,
  drawable_samples=strips,drawable_frames=strip_frames,
  receptor_visits=receptor_visits,receptor_frames=receptor_frames,dropped_receptor_visits=dropped_receptors,
  dropped_model_geometry_samples=dropped_models,dropped_drawable_samples=dropped_strips,
  interactive_observer_error=ITGMANIA_HARNESS_INTERACTIVE_ERROR,
  script_errors=script_errors,dropped_script_errors=dropped_script_errors,
  completion=pending,end_position={beat=beat,seconds=seconds,song_beat=song_beat,
   visible_beat=beat,visible_seconds=visible_seconds},random_seed=cfg.random_seed}))
 file:Close();file:destroy()
end
local function noop() end
ITGMANIA_HARNESS_TRACE={capture_catalog_inputs=function()
  if not catalog_preferences and type(ITGMANIA_HARNESS_CATALOG_INPUTS)=='function' then
   catalog_preferences=ITGMANIA_HARNESS_CATALOG_INPUTS()
  end
 end,
 capture_inputs=function()
  fast_note_rendering=PREFSMAN:GetPreference('FastNoteRendering')
  input_preferences={global_offset_seconds=PREFSMAN:GetPreference('GlobalOffsetSeconds'),
   default_sync_offset=tostring(PREFSMAN:GetPreference('DefaultSyncOffset')),
   visual_delay_seconds=PREFSMAN:GetPreference('VisualDelaySeconds'),
   music_rate=GAMESTATE:GetSongOptionsObject('ModsLevel_Current'):MusicRate()}
  for _,player in ipairs(GAMESTATE:GetEnabledPlayers()) do
   local state=GAMESTATE:GetPlayerState(player)
   local opts=state:GetPlayerOptions('ModsLevel_Current')
   inputs[#inputs+1]={player=tostring(player),options_string=state:GetPlayerOptionsString('ModsLevel_Current'),mini=opts:Mini(),noteskin=opts:NoteSkin(),
    preferred_visual_delay_seconds=state:GetPlayerOptions('ModsLevel_Preferred'):VisualDelay()}
  end
 end,
 seed_random=function() MersenneTwister.Seed(cfg.random_seed) end,
 scene_init=function() scene=scene+1;sampled_frame=-1 end,
 sample_update=sample_update,sample_stage=noop,screen_changed=noop,
 finish=function(reason) pending=reason end}
