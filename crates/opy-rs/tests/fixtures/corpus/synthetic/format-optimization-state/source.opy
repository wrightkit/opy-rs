globalvar g

rule "optimized":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", 1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", g))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 2, g, 4 * 5))
    bigMessage(eventPlayer, "{1} {0}".format(1 + 2, g))
    bigMessage(eventPlayer, "{}".format("{} y".format(1 + 2)))
    bigMessage(eventPlayer, "{}".format(2 ** 3))
    bigMessage(eventPlayer, "{}".format(vect(1, 2, 3)))
    bigMessage(eventPlayer, "{}".format(Hero.REAPER))
    bigMessage(eventPlayer, "{}".format("a" if 1 else "b"))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 1, 2 + 3, g))
    hudText(eventPlayer, "{} x".format(1 + 2), null, null, HudPosition.LEFT, 0, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)

#!disableOptimizations
rule "disabled":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", 1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", g))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 2, g, 4 * 5))
    bigMessage(eventPlayer, "{1} {0}".format(1 + 2, g))
    bigMessage(eventPlayer, "{}".format("{} y".format(1 + 2)))
    bigMessage(eventPlayer, "{}".format(2 ** 3))
    bigMessage(eventPlayer, "{}".format(vect(1, 2, 3)))
    bigMessage(eventPlayer, "{}".format(Hero.REAPER))
    bigMessage(eventPlayer, "{}".format("a" if 1 else "b"))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 1, 2 + 3, g))
    hudText(eventPlayer, "{} x".format(1 + 2), null, null, HudPosition.LEFT, 0, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)

#!enableOptimizations
#!optimizeStrict
rule "strict":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", 1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", g))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 2, g, 4 * 5))
    bigMessage(eventPlayer, "{1} {0}".format(1 + 2, g))
    bigMessage(eventPlayer, "{}".format("{} y".format(1 + 2)))
    bigMessage(eventPlayer, "{}".format(2 ** 3))
    bigMessage(eventPlayer, "{}".format(vect(1, 2, 3)))
    bigMessage(eventPlayer, "{}".format(Hero.REAPER))
    bigMessage(eventPlayer, "{}".format("a" if 1 else "b"))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 1, 2 + 3, g))
    hudText(eventPlayer, "{} x".format(1 + 2), null, null, HudPosition.LEFT, 0, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)

#!disableOptimizeStrict
#!optimizeForSize
rule "size":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", 1 + 2))
    bigMessage(eventPlayer, "{} {}".format("a", g))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 2, g, 4 * 5))
    bigMessage(eventPlayer, "{1} {0}".format(1 + 2, g))
    bigMessage(eventPlayer, "{}".format("{} y".format(1 + 2)))
    bigMessage(eventPlayer, "{}".format(2 ** 3))
    bigMessage(eventPlayer, "{}".format(vect(1, 2, 3)))
    bigMessage(eventPlayer, "{}".format(Hero.REAPER))
    bigMessage(eventPlayer, "{}".format("a" if 1 else "b"))
    bigMessage(eventPlayer, "{} {} {}".format(1 + 1, 2 + 3, g))
    hudText(eventPlayer, "{} x".format(1 + 2), null, null, HudPosition.LEFT, 0, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
