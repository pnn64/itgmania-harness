return Def.ActorFrame{
    OnCommand=function(self)
        assert(self.GetScreenType == nil, "ActorFrame has no Screen methods")
        local screen = SCREENMAN:GetTopScreen()
        assert(screen:GetScreenType() == "ScreenType_Gameplay")
        if screen:GetScreenType() == "ScreenType_Gameplay" then
            screen:GetChild("Overlay"):decelerate(1):diffusealpha(0)
            screen:GetChild("Underlay"):decelerate(1):diffusealpha(0)
        end
    end,
}
