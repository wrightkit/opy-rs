#!translations en fr
#!disableOptimizations

rule "translated long string":
    @Event eachPlayer
    bigMessage(eventPlayer, _("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA {} x".format(1 + 2)))
