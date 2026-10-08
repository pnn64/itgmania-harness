return Def.ActorFrame{
    OnCommand=function(self)
        assert(self:GetNumChildren() == 2)
        assert(self:GetChild("missing") == nil)
        assert(self:GetChild("") == nil)
        assert(self:GetNumChildren() == 2)
        assert(self.GetText == nil)
        assert(self:GetChild("Child").GetText == nil)
        local label = self:GetChild("Label")
        assert(type(label.GetText) == "function")
        assert(label:GetText() == "Alpha")
        mod_actions = {{1, "NativeLookupChecked", true}}
        self:aux(1)
    end,
    Def.ActorFrame{Name="Child"},
    Def.BitmapText{Name="Label", Font="Common Normal", Text="Alpha"}
}
