ActorFrame.CustomProbe = function(self) return self:GetNumChildren() end
return Def.ActorFrame{
 OnCommand=function(self)
  assert(type(self.GetX) == "function" and type(self.Center) == "function")
  assert(self:CustomProbe() == 4)
  self:Center()
  assert(self:GetX() == SCREEN_CENTER_X)
 end,
 Def.ActorFrame{Name="UnknownMethod", InitCommand=function(self)
  assert(self.UnknownMethod == nil, "native actor has no arbitrary method")
 end},
 Def.ActorFrame{Name="ChildAt", InitCommand=function(self)
  assert(self.GetChildAt == nil, "native actor has no GetChildAt")
  assert(ActorFrame.GetChildAt == nil, "native ActorFrame class has no GetChildAt")
 end},
 Def.ActorFrameTexture{Name="Derived", InitCommand=function(self)
  assert(self:CustomProbe() == 0)
  assert(type(self.GetTexture) == "function")
  assert(self.UnknownMethod == nil)
 end},
 Def.Quad{Name="Quad", InitCommand=function(self)
  assert(type(self.Load) == "function")
  assert(self.GetText == nil and self.SetUpdateFunction == nil)
  assert(self.GetChildAt == nil)
 end},
}
