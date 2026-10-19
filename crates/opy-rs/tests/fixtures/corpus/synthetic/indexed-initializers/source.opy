globalvar a 3 = 5
globalvar top 127 = 1
playervar b 7 = 9
subroutine helper 2
subroutine loose = tokens after the separator are dropped by the reference
subroutine pinned 5 = 1 + 2

def helper():
    pass

def loose():
    pass

def pinned():
    pass

rule "uses the declared names":
    @Event eachPlayer
    a += 1
    eventPlayer.b += 1
    helper()
    loose()
    pinned()
