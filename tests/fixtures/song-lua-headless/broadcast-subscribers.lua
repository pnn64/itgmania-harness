local dispatched, shared = {}, { count = 0 }
local outer
local function received(self, params)
    assert(params == outer, "broadcast parameter identity changed")
    params.count = params.count + 1
    dispatched[#dispatched + 1] = self:GetName()
    self:x(params.count)
end
return Def.ActorFrame {
    Name = "parent",
    OnCommand = function(self)
        self:addcommand("DynamicMessage", function(_, params)
            assert(params == shared, "dynamic broadcast parameters changed")
            params.count = params.count + 1
        end)
        self:queuecommand("Send")
    end,
    SendCommand = function(self)
        outer = shared
        assert(MESSAGEMAN:Broadcast("Go", shared) == MESSAGEMAN)
        assert(shared.count == 2 and #dispatched == 2)
        assert(MESSAGEMAN:Broadcast("Dynamic", shared) == MESSAGEMAN)
        assert(shared.count == 3)
        for _, invalid in ipairs({1, true, "text", function() end, self}) do
            assert(not pcall(function() MESSAGEMAN:Broadcast("Go", invalid) end))
        end
        MESSAGEMAN:Broadcast("Outer", shared)
        assert(shared.count == 4)
        SCREENMAN:GetTopScreen():addcommand("ExternalMessage", function(_, params)
            assert(params == shared and params.count == 4)
            params.count = params.count + 1
        end)
        MESSAGEMAN:Broadcast("External", shared)
        assert(shared.count == 5)
        self:queuemessage("Queued")
    end,
    GoMessageCommand = received,
    OuterMessageCommand = function(_, params)
        MESSAGEMAN:Broadcast("Inner", params)
    end,
    QueuedMessageCommand = function(_, params)
        assert(type(params) == "table" and next(params) == nil)
    end,
    Def.Actor {
        Name = "child",
        GoMessageCommand = received,
        InnerMessageCommand = function(_, params)
            assert(params == shared and params.count == 3)
            params.count = params.count + 1
        end,
    },
    Def.Actor {
        Name = "unsubscribed",
        GoCommand = function() error("ordinary commands do not subscribe") end,
    },
}
