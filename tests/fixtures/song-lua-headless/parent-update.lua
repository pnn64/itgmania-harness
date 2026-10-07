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
		self:sleep(0.25):queuecommand("Update")
	end,
	UpdateCommand = function(self)
		self:GetChild("result"):y(self:GetChild("helper"):GetY())
	end,
}
