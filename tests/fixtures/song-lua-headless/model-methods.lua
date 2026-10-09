local piece = "../../../fixtures/actors/model-triangle.txt"
return Def.ActorFrame {
  Def.Model {
    Meshes=piece, Materials=piece, Bones=piece,
    InitCommand=function(self)
      assert(self.UnknownMethod==nil and self.SetVertices==nil)
      assert(type(self.GetNumStates)=="function" and self:GetNumStates()==1)
      assert(not pcall(function() self:loop(1) end))
      assert(not pcall(function() self:SetTextureFiltering(1) end))
      self:loop(true):rate(0.5):position(0):animate(0):play():pause():setstate(0)
      self:texturetranslate(0.2,0.3):texturewrapping(1):SetTextureFiltering(false)
      self:blend("BlendMode_Add"):zbuffer(false):ztest(0)
      self:ztestmode("ZTestMode_WriteOnPass"):zwrite(1):zbias(0.5)
      self:clearzbuffer(0):backfacecull(true):cullmode("CullMode_None")
      self:SetDefaultAnimation("default",1)
      assert(self:GetDefaultAnimation()=="default")
      local result,message = _ITG_MODEL_CALL(999999,"rate",1)
      assert(result==nil and message:match("invalid native Model handle"))
    end,
  },
}
