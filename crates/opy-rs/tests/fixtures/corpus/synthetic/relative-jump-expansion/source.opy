globalvar a
subroutine retained
subroutine unoptimized

macro noop():
    pass

rule "macro passes around a relative jump":
    @Event global
    noop()
    goto loc+2
    noop()
    a = 1
    noop()
    a = 2

def retained():
    goto loc+2
    pass
    a = 1
    a = 2

#!disableOptimizations
rule "macro pass in an unoptimized rule":
    @Event global
    a = 1
    #!enableOptimizations
    noop()
    a = 2

#!disableOptimizations
def unoptimized():
    pass
    a = 1
    goto loc+2
    pass
    a = 2
