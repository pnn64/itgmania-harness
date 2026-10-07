return Def.ActorFrame {
	Name = "root",
	Def.Quad {
		Name = "helper",
		InitCommand = function(self)
			self:y(0)
		end,
	},
	Def.Quad {
		Name = "result",
	},
	OnCommand = function(self)
		local helper = self:GetChild("helper")
		local result = self:GetChild("result")
		mod_message(0.2, function()
			helper:linear(0.5):addy(100)
		end)
		self:SetUpdateFunction(function()
			result:y(helper:GetY())
		end)
	end,
}
