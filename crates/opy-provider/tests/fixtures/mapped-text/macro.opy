globalvar total

macro twice(a):
    total = a
    wait(a + 1)

rule "m":
    @Event global
    twice(3)
    wait(2)
