globalvar a

rule "pass after skip":
    @Event global
    __skip__(2)
    pass
    a = 1
    a = 2

rule "pass after keyword skip":
    @Event global
    __skip__(numberOfActions=1)
    if a == 1:
        pass
    a = 2

rule "pass after skip if zero":
    @Event global
    __skipIf__(a == 1, 0)
    pass
    a = 1
