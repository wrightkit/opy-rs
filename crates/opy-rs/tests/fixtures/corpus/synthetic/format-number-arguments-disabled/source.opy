#!disableOptimizations

globalvar g

#!define pair(a, b) "{} {}".format(a, b)

rule "number arguments":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} {}".format(1, 2))
    bigMessage(eventPlayer, "{} x".format(1 + 2))
    bigMessage(eventPlayer, "{} {}".format(g, 3))
    bigMessage(eventPlayer, pair(5, 1 + 2))
    debug(2)
