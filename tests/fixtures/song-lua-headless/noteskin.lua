local skin = GAMESTATE:GetPlayerState(PLAYER_1):GetPlayerOptions("ModsLevel_Song"):NoteSkin()
return NOTESKIN:LoadActorForNoteSkin("Down", "Explosion", skin) .. {
    OnCommand=function(self)
        assert(skin == "cyber", "native PlayerOptions must retain the initial noteskin")
        self:propagatecommand("Judgment")
        self:propagatecommand("Dim")
        self:propagatecommand("W1")
    end,
}
