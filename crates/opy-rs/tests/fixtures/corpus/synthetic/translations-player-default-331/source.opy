#!translations en fr
#!translateWithPlayerVar

rule "translation player default":
    @Event eachPlayer
    bigMessage(eventPlayer, _("Hello"))
