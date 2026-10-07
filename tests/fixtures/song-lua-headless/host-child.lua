local flag, count, empty, text = ...
assert(select("#", ...) == 4)
assert(flag == false and count == 5 and empty == nil and text == "ok")
return Def.Actor { Name = "Child" }
