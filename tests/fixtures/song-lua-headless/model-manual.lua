local piece = "../../../fixtures/actors/model-triangle.txt"
return Def.ActorFrame {
  Name="Root",
  OnCommand=function(self)
    local model = self:GetChild("Model")
    self:SetDrawFunction(function()
      model:xy(100,200):zoom(2):diffuse(0.5,0.75,1,0.6):glow(1,0,0,0.25):Draw()
      model:x(120):glow(0,0,0,0):Draw()
    end)
  end,
  Def.Model {Name="Model", Meshes=piece, Materials=piece, Bones=piece},
}
