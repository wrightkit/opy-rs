#!include "shared/defs.opy"
#!define SCALE 2.5

playervar P

rule "collect":
    @Event eachPlayer
    for I in range(10):
        total += I * SCALE
        eventPlayer.P = total
    resetScore()
    doubleIt(3)
    debug(Direction.NORTH)
