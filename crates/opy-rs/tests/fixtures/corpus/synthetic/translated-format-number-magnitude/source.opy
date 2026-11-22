#!translations en fr

rule "translated number magnitude":
    @Event eachPlayer
    bigMessage(eventPlayer, _("{0}".format(10000001)))
