return Def.ActorFrame {
	Name = "root",
	Def.Quad {
		Name = "helper",
		InitCommand = function(self)
			self:y(0):linear(1):addy(100)
		end,
	},
	Def.Quad {
		Name = "result",
	},
	OnCommand = function(self)
		local helper = self:GetChild("helper")
		local result = self:GetChild("result")
		self:SetUpdateFunction(function()
			result:y(helper:GetY())
		end)
	end,
}
