return Def.ActorFrame { Def.Quad {
    Name = 'Append',
    OnCommand = function(self)
        self:sleep(2):queuecommand('Append'):linear(0.76):y(-200)
    end,
    AppendCommand = function(self)
        self:accelerate(0.25):addy(350)
            :decelerate(1.5):addy(-500):linear(0.01):y(-150)
    end,
} }
