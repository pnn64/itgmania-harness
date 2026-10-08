local function check_proxy(proxy, target)
    assert(type(ActorProxy) == "table", "ActorProxy public class exists")
    assert(type(ActorProxy.GetX) == "function", "ActorProxy inherits Actor")
    assert(type(ActorProxy.SetTarget) == "function" and type(ActorProxy.GetTarget) == "function")
    assert(proxy.GetChild == nil and proxy.SetUpdateFunction == nil and proxy.GetDrawFunction == nil)
    assert(proxy.UnknownNativeMethod == nil and ActorProxy.UnknownNativeMethod == nil)
    assert(proxy:GetTarget() == nil, "unassigned target is nil")
    assert(proxy:SetTarget(target) == proxy and proxy:GetTarget() == target, "target identity")
    assert(not pcall(function() proxy:SetTarget() end), "missing target is invalid")
    for _, value in ipairs({false, 0, "actor", {}}) do
        assert(not pcall(function() proxy:SetTarget(value) end), "target must be an Actor")
        assert(proxy:GetTarget() == target, "invalid target leaves previous target intact")
    end
    proxy:x(13)
    target:x(7)
    assert(proxy:GetX() == 13 and target:GetX() == 7, "proxy position is independent")
    ActorProxy.ProxyAddedMethod = function(self) return self:GetTarget() end
    assert(proxy:ProxyAddedMethod() == target, "live class methods")
    ActorProxy.ProxyAddedMethod = nil
    local original = Actor.GetX
    Actor.GetX = function() return 91 end
    local inherited = proxy:GetX()
    Actor.GetX = original
    assert(inherited == 91 and proxy:GetX() == 13, "live base overrides")
end

if native_proxy then
    local own = {}
    for name, method in pairs(ActorProxy) do
        if type(method) == "function" then own[name] = true end
    end
    assert(own.SetTarget and own.GetTarget)
    own.SetTarget, own.GetTarget = nil, nil
    assert(next(own) == nil, "exact native own method inventory")
    check_proxy(native_proxy, native_target)
else
    return Def.ActorFrame{
        OnCommand = function(self) check_proxy(self:GetChild("Proxy"), self:GetChild("Target")) end,
        Def.ActorProxy{Name = "Proxy"},
        Def.Actor{Name = "Target"},
    }
end
