#!translations en fr
#!disableOptimizations

globalvar g

rule "translated four arguments":
    @Event eachPlayer
    bigMessage(eventPlayer, _("{} {} {} {}".format("a", "b", "c", 1 + 2)))
    bigMessage(eventPlayer, _("{} {} {} {}".format(g, g, g, 1 + 2)))
