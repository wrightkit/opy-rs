globalvar A = 0
globalvar g
globalvar i
subroutine sub

macro val():
    1 + 2

macro inner():
    g == 1 + 1

macro outer():
    if inner():
        g = 2 * 3

macro setv(v):
    g = v + 0

macro pick():
    if g == 1 + 1:
        g = 1

macro show():
    print(A + 0)
    print(A * 1)

macro pause():
    wait(0.016)
    for i in range(1, 5, 1):
        g = i

#!define CALLPICK pick()

rule "optimized":
    @Event global
    @Condition inner()
    g = val()
    outer()
    setv(3 * 4)
    CALLPICK
    show()
    pause()

#!disableOptimizations
macro late():
    2 + 3

rule "disabled":
    @Event global
    @Condition inner()
    g = val()
    outer()
    setv(3 * 4)
    CALLPICK
    show()

def sub():
    pick()

#!enableOptimizations
#!optimizeStrict
rule "strict":
    @Event global
    show()
    g = late()

#!disableOptimizeStrict
#!optimizeForSize
rule "size":
    @Event global
    pause()
