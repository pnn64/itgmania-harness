local cfg = ITGMANIA_HARNESS_CONFIG
local trace = ITGMANIA_HARNESS_TRACE

-- ScreenSystemLayer survives ScreenGameplay replacement. ScreenChanged is
-- broadcast by native LoadDelayedScreen after activating the next screen.
return Def.ActorFrame {
	LoadActor(cfg.base_system_overlay),
	Def.Actor {
		Name = "ITGmaniaHarnessLifecycleProbe",
		ScreenChangedMessageCommand = function() trace.screen_changed() end,
	},
}
