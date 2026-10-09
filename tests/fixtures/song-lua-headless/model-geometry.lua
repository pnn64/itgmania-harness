local piece = "../../../fixtures/actors/model-triangle.txt"
return Def.ActorFrame {
  Name="Root", FOV=0,
  Def.Model {
    Name="Model", Meshes=piece, Materials=piece, Bones=piece,
    InitCommand=function(self)
      assert(type(self.position)=="function" and type(self.loop)=="function")
      assert(self.UnknownMethod==nil and self.SetVertices==nil)
      assert(not pcall(function() self:loop(1) end), "native loop requires a boolean")
      self:loop(true):rate(1):position(0)
      self:xy(100,200):zoom(2):diffuse(0.5,0.75,1,0.6):glow(1,0,0,0.25)
    end,
    OnCommand=function(self) self:linear(1):xy(120,210) end,
  },
  Def.Model {Name="HiddenModel", Meshes=piece, Materials=piece, Bones=piece,
    InitCommand=function(self) self:visible(false) end},
}
