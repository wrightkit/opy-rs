globalvar g 0

rule "r":
    @Event global
    __skipIf__(g == 1, 1)
    g = 5
    __abortIf__(g == 2)
    g = 6
    __loopIf__(g == 3)
