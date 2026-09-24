globalvar g

subroutine emptySub
subroutine fullSub

def emptySub():
    pass

def fullSub():
    g = 1

rule "empty rule":
    @Event global
    pass

rule "calls":
    @Event global
    emptySub()
    fullSub()
