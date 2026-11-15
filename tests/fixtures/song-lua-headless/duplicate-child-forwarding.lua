return function(root)
 local g=root:GetChild('same'); assert(#g==2 and g~=root:GetChild('same'))
 assert(g:x(17)==g[2] and g[1]:GetX()==0 and g[2]:GetX()==17)
 local getx=g.GetX; g[3]=g[1]; assert(getx(g)==0 and g['3']==nil)
 g[2].GetX=function()return 999 end
 assert(g[2]:GetX()==999 and root:GetChild('same'):GetX()==17)
 local original=Actor.GetX
 Actor.GetX=function()return 123 end
 assert(getx(g)==0 and g:GetX()==123)
 Actor.GetX=original
 Actor.multi=function(self,...)return self,... end
 local saved=root:GetChild('same').multi
 Actor.multi=function()return 'replaced' end
 local t={}; local a,b,c,d=saved(root:GetChildren().same,t,nil,42)
 assert(a==g[2] and b==t and c==nil and d==42)
 assert(select('#',saved(root:GetChild('same'),nil))==2)
 assert(root:GetChild('same'):multi()=='replaced')
 Actor.multi=nil
 assert(type(g.MissingMethod)=='function' and not pcall(g.MissingMethod,g))
 g[1]:name('changed');assert(root:GetChild('changed')==g[1] and root:GetChild('same')==g[2])
end
