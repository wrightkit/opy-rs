#!optimizeForSize
globalvar g
playervar pv

rule "builtins":
    @Event eachPlayer
    chase(eventPlayer.pv, false, rate=true, ChaseReeval.NONE)
    chase(eventPlayer.pv, 0, rate=1, ChaseReeval.NONE)
    eventPlayer.applyImpulse(Vector.UP, 1, Relativity.TO_WORLD)
    eventPlayer.startThrottleInDirection(Vector.UP, g, Relativity.TO_WORLD)
    createIcon(getAllPlayers(), vect(1, 2, 3), Icon.FLAG, IconReeval.VISIBILITY_AND_POSITION, Color.BLUE)
    hudText(getAllPlayers(), null, "h {}".format(g), "t {} {} {} {}".format(g, g, g, g + 1), HudPosition.LEFT, 6, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)
    hudText(getAllPlayers(), "h", null, g if g else "z", HudPosition.TOP, 0, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)
    if eventPlayer.isFiringSecondaryFire():
        g = 1
    if getCurrentGamemode() != Gamemode.SKIRMISH:
        g = 2
