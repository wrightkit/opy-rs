globalvar g

rule "boolean arguments":
    @Event eachPlayer
    bigMessage(eventPlayer, "{}".format(not true))
    bigMessage(eventPlayer, "{} x".format(true))
    bigMessage(eventPlayer, "{} x".format(false))
    bigMessage(eventPlayer, "{} x".format(not false))
    bigMessage(eventPlayer, "{} x".format(1 == 1))
    bigMessage(eventPlayer, "{} x".format(not (1 == 2)))
    bigMessage(eventPlayer, "{} x".format(3 > 2))
    bigMessage(eventPlayer, "{} x".format(true and false))
    bigMessage(eventPlayer, "{} x".format(true or false))
    bigMessage(eventPlayer, "{} x".format(not 1))
    bigMessage(eventPlayer, "{} x".format(not "a"))
    bigMessage(eventPlayer, "{} x".format(1 in [1, 2]))
    bigMessage(eventPlayer, "{} {}".format(not true, 1 + 2))
    bigMessage(eventPlayer, "{} {} {}".format(true, g, 1 + 2))
    bigMessage(eventPlayer, "{1} {0}".format(false, g))
    bigMessage(eventPlayer, "{}".format("{} y".format(true)))

rule "non-boolean constants fold":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(1 + 2))
    bigMessage(eventPlayer, "{} x".format("a"))
    bigMessage(eventPlayer, "{} x".format(vect(1, 2, 3)))
    bigMessage(eventPlayer, "{} x".format(Hero.REAPER))

#!optimizeStrict
rule "strict":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(not true))
    bigMessage(eventPlayer, "{} x".format(1 == 1))

#!disableOptimizeStrict
#!optimizeForSize
rule "size":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(not true))
    bigMessage(eventPlayer, "{} x".format(1 == 1))

#!disableOptimizations
rule "disabled":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} x".format(not true))
    bigMessage(eventPlayer, "{} x".format(1 == 1))
