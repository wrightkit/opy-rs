#!translations en fr
#!translateWithPlayerVar noTlErr

rule "translation player no tlerr":
    @Event eachPlayer
    bigMessage(eventPlayer, _("Hello"))
