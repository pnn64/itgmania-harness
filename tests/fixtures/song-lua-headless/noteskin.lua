return NOTESKIN:LoadActorForNoteSkin("Down", "Explosion", "cyber") .. {
    OnCommand=function(self)
        self:propagatecommand("Judgment")
        self:propagatecommand("Dim")
        self:propagatecommand("W1")
    end,
}
